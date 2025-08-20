# HiveMind 開發者指南

## 概述

歡迎來到 HiveMind 分布式運算平台的開發者指南！本文檔將幫助您了解項目架構、開發環境設置、編碼規範和貢獻流程。

## 項目架構

### 整體架構圖

```
┌─────────────────────────────────────────────────────────────┐
│                    HiveMind 分布式運算平台                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │   Master    │◄──►│ Node Pool   │◄──►│   Worker    │      │
│  │             │    │             │    │             │      │
│  │ ┌─────────┐ │    │ ┌─────────┐ │    │ ┌─────────┐ │      │
│  │ │Web UI   │ │    │ │Resource │ │    │ │Task     │ │      │
│  │ │gRPC API │ │    │ │Manager  │ │    │ │Executor │ │      │
│  │ │VPN Mgmt │ │    │ │Scheduler│ │    │ │Monitor  │ │      │
│  │ └─────────┘ │    │ │Rewards  │ │    │ │Reporter │ │      │
│  └─────────────┘    │ └─────────┘ │    │ └─────────┘ │      │
│                     └─────────────┘    └─────────────┘      │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │     AI      │    │     BT      │    │    Web      │      │
│  │             │    │             │    │             │      │
│  │ 模型分割     │    │ P2P傳輸     │    │ 官方網站     │      │
│  │ 智能調度     │    │ 種子管理     │    │ 用戶註冊     │      │
│  │ (開發中)     │    │ (已完成)     │    │ (已上線)     │      │
│  └─────────────┘    └─────────────┘    └─────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 模組依賴關係

### 實際專案結構

```
hivemind/
├── 📁 node_pool/                   # 節點池服務（核心組件）
│   ├── node_pool_server.py      # 主服務器入口
│   ├── user_service.py          # 用戶認證服務
│   ├── node_manager_service.py  # 節點管理服務
│   ├── master_node_service.py   # 主控節點服務
│   ├── database_manager.py      # 資料庫管理
│   ├── config.py               # 配置管理
│   ├── nodepool_pb2.py         # gRPC 協議定義
│   └── nodepool_pb2_grpc.py    # gRPC 服務存根
│
├── 📁 worker/                      # 工作節點
│   ├── worker_node.py           # 主工作節點程式（1798行）
│   ├── nodepool_pb2.py         # gRPC 協議
│   ├── nodepool_pb2_grpc.py    # gRPC 服務
│   ├── Dockerfile             # Docker 建置檔案
│   ├── 📁 templates/              # Flask 網頁模板
│   ├── 📁 static/                 # 靜態資源
│   └── 📁 hivemind_worker/        # Worker 套件
│
├── 📁 master/                      # 主控節點
│   ├── master_node.py           # 主控節點程式（679行）
│   ├── nodepool_pb2.py         # gRPC 協議
│   ├── nodepool_pb2_grpc.py    # gRPC 服務
│   ├── 📁 templates_master/        # 主控節點網頁模板
│   └── 📁 static_master/          # 主控節點靜態資源
│
├── 📁 web/                         # 官方網站
│   ├── app.py                  # Flask 網站應用（860行）
│   ├── vpn_service.py          # VPN 服務
│   ├── wireguard_server.py     # WireGuard 伺服器
│   ├── 📁 templates/              # 網站模板
│   └── 📁 static/                 # 網站靜態資源
│
├── 📁 ai/                          # AI 模組（開發中）
│   ├── main.py                 # 主程式（空檔案）
│   ├── breakdown.py            # 模型分解程式（300行）
│   ├── Identification.py       # 模型識別
│   └── q_table.pkl             # Q-learning 表
│
├── 📁 bt/                          # BitTorrent P2P 模組
│   ├── create_torrent.py       # 建立種子檔案（78行）
│   ├── tracker.py              # BitTorrent 追蹤器
│   ├── seeder.py               # 種子播種器
│   └── test.torrent            # 測試種子檔案
│
├── 📁 taskworker/                  # 任務執行器
│   ├── worker.py               # 任務工作器
│   ├── storage.py              # 儲存管理
│   ├── dns_proxy.py            # DNS 代理
│   ├── rpc_service.py          # RPC 服務
│   └── 📁 protos/                 # Protocol Buffers 定義
│
└── 📁 docs/                        # 文檔目錄
    ├── API.md                  # API 文檔
    ├── DEPLOYMENT.md           # 部署指南
    ├── TROUBLESHOOTING.md      # 故障排除
    └── DEVELOPER.md            # 開發者指南（本檔案）
```

### 核心架構設計

```
┌─────────────────────────────────────────────────────────────┐
│                    HiveMind 分布式運算平台                     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │   Master    │◄──►│ Node Pool   │◄──►│   Worker    │      │
│  │ master_node │    │ node_pool_  │    │ worker_node │      │
│  │ .py (679行) │    │ server.py   │    │ .py(1798行) │      │
│  │             │    │             │    │             │      │
│  │ ┌─────────┐ │    │ ┌─────────┐ │    │ ┌─────────┐ │      │
│  │ │Flask UI │ │    │ │gRPC API │ │    │ │Docker   │ │      │
│  │ │Task Mgmt│ │    │ │User Auth│ │    │ │Executor │ │      │
│  │ │VPN Mgmt │ │    │ │Node Mgmt│ │    │ │Monitor  │ │      │
│  │ └─────────┘ │    │ │Database │ │    │ │Flask UI │ │      │
│  └─────────────┘    │ └─────────┘ │    │ └─────────┘ │      │
│                     └─────────────┘    └─────────────┘      │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │     AI      │    │     BT      │    │    Web      │      │
│  │ breakdown.py│    │create_torrent│    │   app.py    │      │
│  │ (300行)     │    │  .py (78行) │    │  (860行)    │      │
│  │ Q-learning  │    │ BitTorrent  │    │ 官方網站     │      │
│  │ 模型分解     │    │ P2P傳輸     │    │ 用戶註冊     │      │
│  │ (開發中)     │    │ (已實現)     │    │ VPN管理     │      │
│  └─────────────┘    └─────────────┘    └─────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### 實際技術架構

#### gRPC 通訊架構
基於實際的 `nodepool_pb2.py` 和 `nodepool_pb2_grpc.py` 檔案：

```python
# 主要服務介面（基於實際程式碼）
class UserServiceServicer:           # 用戶認證服務
    def Login(self, request, context)
    def Register(self, request, context)
    def Transfer(self, request, context)
    def GetBalance(self, request, context)

class NodeManagerServiceServicer:    # 節點管理服務
    def RegisterNode(self, request, context)
    def ReportStatus(self, request, context)
    def GetTask(self, request, context)
    def SubmitResult(self, request, context)

class MasterNodeServiceServicer:     # 主控節點服務
    def SubmitTask(self, request, context)
    def GetTaskStatus(self, request, context)
    def ListTasks(self, request, context)
    def CancelTask(self, request, context)
```

### 模組依賴關係

## 開發環境設置

### 系統要求

根據實際專案依賴（requirements.txt）：

- **作業系統**: Ubuntu 20.04+ / macOS 11+ / Windows 10+
- **Python**: 3.8+ （必須）
- **Docker**: 20.10+ （必須，Worker 節點需要）
- **Git**: 2.25+

### 實際依賴項目

基於 `requirements.txt` 的核心依賴：

```bash
# 核心依賴（實際專案使用）
docker==7.1.0              # Docker Python SDK
Flask==3.0.3               # Web 框架（用於 UI）
grpcio==1.64.1             # gRPC 核心庫
grpcio-tools==1.64.1       # gRPC 工具（Protocol Buffers）
netifaces==0.11.0          # 網路介面檢測
psutil==5.9.8              # 系統資源監控
requests==2.32.3           # HTTP 請求庫
bcrypt                     # 密碼加密
pyjwt                      # JWT 令牌處理
```

### AI 模組額外依賴

基於 `ai/breakdown.py` 的實際使用：

```bash
# AI 模組依賴
torch                      # PyTorch 深度學習框架
numpy                      # 數值計算
pickle                     # 物件序列化（用於 q_table.pkl）
```

### BT 模組額外依賴

基於 `bt/create_torrent.py` 的實際使用：

```bash
# BitTorrent 模組依賴
libtorrent-rasterbar      # BitTorrent 函式庫
```

### 開發工具推薦

- **IDE**: VS Code / PyCharm
- **版本控制**: Git + GitHub
- **容器化**: Docker + Docker Compose
- **文檔**: Markdown（目前使用）

**測試狀態**: 目前項目尚未建立測試框架，這是一個待開發的重要功能。

### 環境配置

1. **克隆專案**
   ```bash
   git clone https://github.com/him6794/hivemind.git
   cd hivemind
   ```

2. **創建開發分支**
   ```bash
   git checkout -b feature/your-feature-name
   ```

3. **設置 Python 虛擬環境**
   ```bash
   python3 -m venv venv
   source venv/bin/activate  # Linux/macOS
   # 或 venv\Scripts\activate  # Windows
   ```

4. **安裝開發依賴**
   ```bash
   pip install -r requirements.txt
   pip install -r requirements-dev.txt  # 開發專用依賴
   ```

5. **配置 pre-commit 鉤子**
   ```bash
   pre-commit install
   ```

6. **設置環境變數**
   ```bash
   cp .env.example .env.dev
   # 編輯 .env.dev 設置開發環境配置
   ```

### Docker 開發環境

```yaml
# docker-compose.dev.yml
version: '3.8'
services:
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
  
  postgres:
    image: postgres:15-alpine
    environment:
      POSTGRES_DB: hivemind_dev
      POSTGRES_USER: hivemind
      POSTGRES_PASSWORD: dev_password
    ports:
      - "5432:5432"
  
  node-pool-dev:
    build:
      context: ./node_pool
      dockerfile: Dockerfile.dev
    volumes:
      - ./node_pool:/app
      - ./proto:/app/proto
    ports:
      - "50051:50051"
    environment:
      - ENV=development
      - REDIS_URL=redis://redis:6379/0
    depends_on:
      - redis
      - postgres
```

## 代碼結構

### 目錄組織

```
hivemind/
├── ai/                     # AI 模組
│   ├── breakdown.py        # 模型分割
│   ├── identification.py   # 模型識別
│   └── main.py            # 主程序
├── bt/                     # BitTorrent 模組
│   ├── create_torrent.py   # 種子創建
│   ├── seeder.py          # 種子上傳
│   └── tracker.py         # 追蹤器
├── docs/                   # 文檔目錄
│   ├── API.md
│   ├── DEPLOYMENT.md
│   └── TROUBLESHOOTING.md
├── master/                 # 主控節點
│   ├── master_node.py      # 主程序
│   ├── vpn.py             # VPN 管理
│   └── templates/         # Web 模板
├── node_pool/              # 節點池
│   ├── node_pool_server.py # gRPC 服務器
│   ├── node_manager.py     # 節點管理
│   ├── user_manager.py     # 用戶管理
│   └── database_manager.py # 數據庫管理
├── worker/                 # 工作節點
│   ├── worker_node.py      # 主程序
│   ├── build.py           # 構建腳本
│   └── templates/         # Web 模板
├── web/                    # 官方網站
│   ├── app.py             # Flask 應用
│   └── static/            # 靜態資源
├── requirements.txt        # Python 依賴
└── docker-compose.yml      # Docker 配置
```

### 實際程式碼組織

#### Node Pool (節點池服務)

**主要檔案**: `node_pool/node_pool_server.py` (52行)

```python
# 實際的服務器架構
def serve():
    server = grpc.server(
        futures.ThreadPoolExecutor(max_workers=20),
        options=[
            ('grpc.max_receive_message_length', 100 * 1024 * 1024),  # 100MB
            ('grpc.max_send_message_length', 100 * 1024 * 1024),     # 100MB
        ]
    )
    
    # 實際的服務實例
    user_service = UserServiceServicer()
    node_manager_service = NodeManagerServiceServicer()
    master_node_service = MasterNodeServiceServicer()
```

**實際服務類** (基於 `node_pool/user_service.py`, `node_manager_service.py`, `master_node_service.py`):

```python
class UserServiceServicer(nodepool_pb2_grpc.UserServiceServicer):
    """用戶認證和代幣管理服務"""
    
    def Login(self, request, context):
        """用戶登入"""
        pass
        
    def Register(self, request, context):
        """用戶註冊"""
        pass
        
    def Transfer(self, request, context):
        """代幣轉帳"""
        pass

class NodeManagerServiceServicer(nodepool_pb2_grpc.NodeManagerServiceServicer):
    """節點管理服務"""
    
    def RegisterNode(self, request, context):
        """節點註冊"""
        pass
        
    def ReportStatus(self, request, context):
        """狀態回報"""
        pass
```

#### Worker Node (工作節點)

**主要檔案**: `worker/worker_node.py` (1798行)

```python
# Worker 節點的實際類別結構
class WorkerNode:
    def __init__(self):
        self.node_id = str(uuid4())
        self.status = "Initializing"
        self.running_tasks = {}  # 多任務支援
        self.task_locks = {}     # 任務鎖
        self.username = None
        self.token = None
        self.cpt_balance = 0
        self.trust_score = 0
        self.trust_group = "low"
        
        # 資源管理
        self.available_resources = {
            "cpu": 0,
            "memory_gb": 0,
            "gpu": 0,
            "gpu_memory_gb": 0
        }
        
        # 初始化組件
        self._init_hardware()    # 硬體檢測
        self._init_docker()      # Docker 連接
        self._init_grpc()        # gRPC 客戶端
        self._init_flask()       # Web 界面
```

#### Master Node (主控節點)

**主要檔案**: `master/master_node.py` (679行)

```python
# Master 節點的實際類別結構
class MasterNodeUI:
    def __init__(self, grpc_address):
        self.grpc_address = grpc_address
        self.channel = None
        self.user_stub = None
        self.master_stub = None
        self.node_stub = None
        self.token = None
        self.task_status_cache = {}
        self.user_list = []       # 用戶會話管理
        
        # Flask 應用設置
        self.app = Flask(__name__, 
                        template_folder="templates_master",
                        static_folder="static_master")
        self.setup_flask_routes()
```

#### AI Module (AI 模組)

**主要檔案**: `ai/breakdown.py` (300行)

```python
# AI 模型分解的實際實現
CONFIG = {
    'learning_rate': 0.1,
    'discount_factor': 0.95,
    'episodes_per_cycle': 5,
    'max_steps': 5,
    'timeout_seconds': 10,
}

# Q-learning 實現（實際程式碼）
class QLearningAgent:
    def __init__(self):
        self.q_table = {}  # 存儲在 q_table.pkl
        
    def choose_action(self, state):
        # Q-learning 決策邏輯
        pass
        
    def update_q_table(self, state, action, reward, next_state):
        # Q 表更新
        pass
```

#### BT Module (BitTorrent 模組)

**主要檔案**: `bt/create_torrent.py` (78行)

```python
# BitTorrent 種子創建的實際實現
def create_private_torrent(file_or_dir_path, tracker_url, output_torrent_path):
    """為指定檔案或資料夾建立私有 .torrent 檔案"""
    
    # 使用 libtorrent 庫
    fs = lt.file_storage()
    lt.add_files(fs, target_path)
    
    # 建立 torrent
    t = lt.create_torrent(fs, 0)
    t.add_tracker(tracker_url)
    t.set_creator("HiveMind BT Module")
    t.set_private(True)  # 私有種子
```

## 編碼規範

### Python 編碼標準

我們遵循 **PEP 8** 編碼規範，並有以下額外要求：

1. **命名約定**
   ```python
   # 類名使用 PascalCase
   class NodeManager:
       pass
   
   # 函數和變數使用 snake_case
   def register_node():
       node_id = "worker_001"
   
   # 常數使用 UPPER_CASE
   MAX_WORKERS = 100
   DEFAULT_TIMEOUT = 30
   ```

2. **類型註解**
   ```python
   from typing import List, Optional, Dict, Union
   
   def process_nodes(
       nodes: List[Node], 
       filters: Optional[Dict[str, str]] = None
   ) -> List[Node]:
       """處理節點列表，支援可選過濾器"""
       if filters is None:
           filters = {}
       return [node for node in nodes if match_filters(node, filters)]
   ```

3. **文檔字符串**
   ```python
   def calculate_reward(
       node_id: str, 
       task_duration: int, 
       resource_usage: Dict[str, float]
   ) -> float:
       """
       計算節點獎勵金額
       
       Args:
           node_id: 節點唯一識別符
           task_duration: 任務執行時間 (秒)
           resource_usage: 資源使用情況字典
           
       Returns:
           計算得出的獎勵金額 (CPT)
           
       Raises:
           ValueError: 當參數無效時
       """
       if task_duration <= 0:
           raise ValueError("任務執行時間必須大於 0")
       
       # 計算邏輯...
       return reward_amount
   ```

4. **錯誤處理**
   ```python
   import logging
   
   logger = logging.getLogger(__name__)
   
   def safe_operation():
       try:
           result = risky_operation()
           return result
       except SpecificException as e:
           logger.error(f"操作失敗: {e}")
           raise
       except Exception as e:
           logger.exception("未預期的錯誤")
           raise RuntimeError(f"內部錯誤: {e}") from e
   ```

### gRPC 開發規範

1. **Protocol Buffers 定義**
   ```protobuf
   syntax = "proto3";
   
   package hivemind.v1;
   
   option go_package = "github.com/hivemind/proto/v1";
   option java_package = "com.hivemind.proto.v1";
   option csharp_namespace = "HiveMind.Proto.V1";
   
   // 使用清晰的命名
   message RegisterNodeRequest {
     string node_id = 1;           // 必填字段在前
     string hostname = 2;
     optional string location = 3; // 可選字段在後
   }
   ```

2. **服務實現**
   ```python
   class NodeServicer(nodepool_pb2_grpc.NodeServiceServicer):
       """節點服務實現"""
       
       def RegisterNode(
           self, 
           request: nodepool_pb2.RegisterNodeRequest, 
           context: grpc.ServicerContext
       ) -> nodepool_pb2.RegisterNodeResponse:
           """節點註冊服務"""
           try:
               # 驗證請求
               if not request.node_id:
                   context.set_code(grpc.StatusCode.INVALID_ARGUMENT)
                   context.set_details("節點 ID 不能為空")
                   return nodepool_pb2.RegisterNodeResponse()
               
               # 業務邏輯...
               result = self.node_manager.register_node(request)
               
               return nodepool_pb2.RegisterNodeResponse(
                   success=True,
                   message="註冊成功",
                   node_token=result.token
               )
               
           except Exception as e:
               logger.exception("節點註冊失敗")
               context.set_code(grpc.StatusCode.INTERNAL)
               context.set_details(str(e))
               return nodepool_pb2.RegisterNodeResponse()
   ```

### 前端開發規範 (Web UI)

1. **HTML 模板**
   ```html
   <!-- templates/base.html -->
   <!DOCTYPE html>
   <html lang="zh-TW">
   <head>
       <meta charset="UTF-8">
       <meta name="viewport" content="width=device-width, initial-scale=1.0">
       <title>{% block title %}HiveMind{% endblock %}</title>
       <link href="https://cdn.jsdelivr.net/npm/bootstrap@5.1.3/dist/css/bootstrap.min.css" rel="stylesheet">
   </head>
   <body>
       {% block content %}{% endblock %}
       <script src="https://cdn.jsdelivr.net/npm/bootstrap@5.1.3/dist/js/bootstrap.bundle.min.js"></script>
   </body>
   </html>
   ```

2. **JavaScript 規範**
   ```javascript
   // static/js/dashboard.js
   class Dashboard {
       constructor() {
           this.apiClient = new HiveMindAPIClient();
           this.updateInterval = 5000; // 5 秒更新一次
           this.init();
       }
       
       async init() {
           await this.loadNodeStatus();
           this.startAutoUpdate();
       }
       
       async loadNodeStatus() {
           try {
               const nodes = await this.apiClient.getNodes();
               this.renderNodes(nodes);
           } catch (error) {
               console.error('載入節點狀態失敗:', error);
               this.showError('無法載入節點狀態');
           }
       }
   }
   
   // 初始化
   document.addEventListener('DOMContentLoaded', () => {
       new Dashboard();
   });
   ```

## 測試規範

**重要提醒**: 本專案目前尚未建立測試框架和測試文件。建立完整的測試體系是未來開發的重要目標。

建議的測試框架規劃：
- **單元測試**: 針對各個模組功能的測試
- **整合測試**: 測試模組間的協作
- **端到端測試**: 測試完整的使用者工作流程
- **性能測試**: 測試系統在高負載下的表現

## 文檔規範

### API 文檔

使用 **Sphinx** 自動生成 API 文檔：

```python
"""
Node Pool API

.. module:: node_pool.api
   :synopsis: HiveMind Node Pool API 模組

.. moduleauthor:: HiveMind Team <dev@hivemind.com>
"""

class NodeManager:
    """
    節點管理器
    
    這個類負責管理所有工作節點的註冊、狀態更新和資源分配。
    
    :param redis_client: Redis 客戶端實例
    :type redis_client: redis.Redis
    :param config: 配置對象
    :type config: Config
    
    Example:
        >>> from node_pool.node_manager import NodeManager
        >>> manager = NodeManager(redis_client, config)
        >>> result = manager.register_node(node_info)
        >>> print(result.success)
        True
    """
    
    def register_node(self, node_info: dict) -> RegisterResult:
        """
        註冊新的工作節點
        
        :param node_info: 節點資訊字典
        :type node_info: dict
        :returns: 註冊結果
        :rtype: RegisterResult
        :raises ValueError: 當節點資訊無效時
        :raises RuntimeError: 當 Redis 連接失敗時
        
        :Example:
        
        >>> node_info = {
        ...     'node_id': 'worker_001',
        ...     'hostname': 'worker-host',
        ...     'cpu_cores': 8,
        ...     'memory_gb': 16.0
        ... }
        >>> result = manager.register_node(node_info)
        >>> print(result.node_token)
        'eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9...'
        """
        pass
```

### README 文檔

每個模組都應該有自己的 README.md：

```markdown
# Node Pool 模組

## 概述
節點池是 HiveMind 的核心調度組件...

## 快速開始
\`\`\`python
from node_pool import NodeManager
manager = NodeManager()
\`\`\`

## API 參考
- [NodeManager](docs/api/node_manager.md)
- [TaskScheduler](docs/api/task_scheduler.md)

## 配置
詳見 [配置指南](docs/configuration.md)
```

## 版本控制和發布

### Git 工作流程

我們使用 **Git Flow** 分支策略：

```
main (生產)
├── develop (開發主分支)
│   ├── feature/user-authentication
│   ├── feature/task-scheduling
│   └── feature/node-monitoring
├── release/v1.2.0 (預發布)
└── hotfix/critical-bug-fix (緊急修復)
```

### 提交訊息規範

使用 **Conventional Commits** 格式：

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

**類型說明**：
- `feat`: 新功能
- `fix`: 修復問題
- `docs`: 文檔更新
- `style`: 代碼格式調整
- `refactor`: 代碼重構
- `perf`: 性能優化
- `test`: 測試相關
- `chore`: 雜項工作

**示例**：
```
feat(node-pool): 添加節點信任等級評估

實現了基於 Docker 狀態和歷史任務完成率的
節點信任等級評估系統。

- 添加 TrustLevelCalculator 類
- 實現三級信任評估（HIGH/MEDIUM/LOW）
- 更新節點註冊流程以包含信任評估

Closes #123
```

### 版本號規範

使用 **語義化版本** (Semantic Versioning)：

- `MAJOR.MINOR.PATCH` (例如：1.2.3)
- **MAJOR**: 不兼容的 API 變更
- **MINOR**: 向後兼容的功能新增
- **PATCH**: 向後兼容的錯誤修復

### 發布流程

1. **創建發布分支**
   ```bash
   git checkout develop
   git pull origin develop
   git checkout -b release/v1.2.0
   ```

2. **更新版本號**
   ```bash
   # 更新 setup.py, __init__.py 等文件中的版本號
   echo "1.2.0" > VERSION
   ```

3. **驗證功能正常（測試框架待建立）**
   ```bash
   # 手動驗證核心功能運作正常
   # 測試框架建立後，將執行自動化測試
   ```

4. **更新 CHANGELOG**
   ```markdown
   ## [1.2.0] - 2024-01-15
   
   ### Added
   - 節點信任等級評估系統
   - 任務優先級調度算法
   - Web 界面實時監控功能
   
   ### Changed
   - 優化 gRPC 性能，減少 30% 響應時間
   - 改進錯誤處理和日誌記錄
   
   ### Fixed
   - 修復並發節點註冊時的競態條件
   - 解決 VPN 配置生成問題
   ```

5. **合併到主分支**
   ```bash
   git checkout main
   git merge --no-ff release/v1.2.0
   git tag -a v1.2.0 -m "Release version 1.2.0"
   git push origin main --tags
   ```

6. **創建 GitHub Release**
   - 在 GitHub 上創建新的 Release
   - 上傳構建產物和文檔
   - 發布更新通知

## 性能優化

### 代碼性能

1. **使用性能分析工具**
   ```python
   import cProfile
   import pstats
   
   def profile_function(func):
       """性能分析裝飾器"""
       def wrapper(*args, **kwargs):
           pr = cProfile.Profile()
           pr.enable()
           result = func(*args, **kwargs)
           pr.disable()
           
           stats = pstats.Stats(pr)
           stats.sort_stats('cumulative')
           stats.print_stats(10)  # 顯示前 10 個最耗時的函數
           
           return result
       return wrapper
   
   @profile_function
   def expensive_operation():
       # 耗時操作...
       pass
   ```

2. **異步編程**
   ```python
   import asyncio
   import aioredis
   from typing import List
   
   class AsyncNodeManager:
       def __init__(self):
           self.redis = None
       
       async def init_redis(self):
           self.redis = await aioredis.create_redis_pool(
               'redis://localhost:6379',
               minsize=5,
               maxsize=20
           )
       
       async def register_multiple_nodes(
           self, 
           nodes: List[dict]
       ) -> List[bool]:
           """並行註冊多個節點"""
           tasks = [
               self.register_node_async(node) 
               for node in nodes
           ]
           results = await asyncio.gather(*tasks, return_exceptions=True)
           return [isinstance(r, bool) and r for r in results]
   ```

### 數據庫優化

1. **Redis 性能調優**
   ```python
   import redis
   from redis import ConnectionPool
   
   # 使用連接池
   pool = ConnectionPool(
       host='localhost',
       port=6379,
       db=0,
       max_connections=20,
       retry_on_timeout=True,
       socket_keepalive=True,
       socket_keepalive_options={}
   )
   
   redis_client = redis.Redis(connection_pool=pool)
   
   # 批量操作
   def batch_update_nodes(node_updates):
       pipe = redis_client.pipeline()
       for node_id, data in node_updates.items():
           pipe.hset(f"node:{node_id}", mapping=data)
       pipe.execute()
   ```

2. **SQL 查詢優化**
   ```sql
   -- 為常用查詢添加索引
   CREATE INDEX idx_users_username ON users(username);
   CREATE INDEX idx_nodes_status_trust ON nodes(status, trust_level);
   CREATE INDEX idx_tasks_created_priority ON tasks(created_at, priority);
   
   -- 使用 EXPLAIN 分析查詢計畫
   EXPLAIN ANALYZE SELECT * FROM nodes 
   WHERE status = 'ACTIVE' AND trust_level = 'HIGH'
   ORDER BY last_seen DESC LIMIT 10;
   ```

### 網絡優化

1. **gRPC 優化**
   ```python
   import grpc
   from concurrent import futures
   
   # 服務器優化
   server = grpc.server(
       futures.ThreadPoolExecutor(max_workers=50),
       options=[
           ('grpc.keepalive_time_ms', 30000),
           ('grpc.keepalive_timeout_ms', 5000),
           ('grpc.keepalive_permit_without_calls', True),
           ('grpc.http2.max_pings_without_data', 0),
           ('grpc.http2.min_time_between_pings_ms', 10000),
           ('grpc.http2.min_ping_interval_without_data_ms', 300000),
           ('grpc.max_receive_message_length', 100 * 1024 * 1024),  # 100MB
           ('grpc.max_send_message_length', 100 * 1024 * 1024),     # 100MB
       ]
   )
   
   # 客戶端優化
   channel = grpc.insecure_channel(
       'localhost:50051',
       options=[
           ('grpc.keepalive_time_ms', 30000),
           ('grpc.keepalive_timeout_ms', 5000),
           ('grpc.keepalive_permit_without_calls', True),
       ]
   )
   ```

## 安全考慮

### 輸入驗證

```python
from typing import Dict, Any
import re

class InputValidator:
    """輸入驗證器"""
    
    @staticmethod
    def validate_node_id(node_id: str) -> bool:
        """驗證節點 ID 格式"""
        pattern = r'^[a-zA-Z0-9_-]{3,32}$'
        return bool(re.match(pattern, node_id))
    
    @staticmethod
    def validate_user_input(data: Dict[str, Any]) -> Dict[str, str]:
        """驗證用戶輸入，返回錯誤字典"""
        errors = {}
        
        if 'username' in data:
            username = data['username']
            if not isinstance(username, str) or len(username) < 3:
                errors['username'] = '用戶名至少需要 3 個字符'
            elif not re.match(r'^[a-zA-Z0-9_]+$', username):
                errors['username'] = '用戶名只能包含字母、數字和下劃線'
        
        if 'email' in data:
            email = data['email']
            email_pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$'
            if not re.match(email_pattern, email):
                errors['email'] = '無效的電子郵件格式'
        
        return errors
```

### 身份驗證和授權

```python
import jwt
import bcrypt
from datetime import datetime, timedelta
from functools import wraps

class AuthManager:
    """身份驗證管理器"""
    
    def __init__(self, secret_key: str):
        self.secret_key = secret_key
        self.algorithm = 'HS256'
        self.token_expiry = timedelta(hours=1)
    
    def hash_password(self, password: str) -> str:
        """哈希密碼"""
        salt = bcrypt.gensalt()
        return bcrypt.hashpw(password.encode('utf-8'), salt).decode('utf-8')
    
    def verify_password(self, password: str, hashed: str) -> bool:
        """驗證密碼"""
        return bcrypt.checkpw(
            password.encode('utf-8'), 
            hashed.encode('utf-8')
        )
    
    def generate_token(self, user_id: str, permissions: list) -> str:
        """生成 JWT 令牌"""
        payload = {
            'user_id': user_id,
            'permissions': permissions,
            'exp': datetime.utcnow() + self.token_expiry,
            'iat': datetime.utcnow()
        }
        return jwt.encode(payload, self.secret_key, algorithm=self.algorithm)
    
    def verify_token(self, token: str) -> dict:
        """驗證 JWT 令牌"""
        try:
            payload = jwt.decode(
                token, 
                self.secret_key, 
                algorithms=[self.algorithm]
            )
            return payload
        except jwt.ExpiredSignatureError:
            raise ValueError('令牌已過期')
        except jwt.InvalidTokenError:
            raise ValueError('無效的令牌')

def require_auth(required_permission: str = None):
    """身份驗證裝飾器"""
    def decorator(func):
        @wraps(func)
        def wrapper(*args, **kwargs):
            # 從請求中獲取令牌
            token = extract_token_from_request()
            
            try:
                payload = auth_manager.verify_token(token)
                
                # 檢查權限
                if required_permission:
                    if required_permission not in payload.get('permissions', []):
                        raise PermissionError('權限不足')
                
                # 將用戶信息注入到請求中
                kwargs['current_user'] = payload
                return func(*args, **kwargs)
                
            except ValueError as e:
                raise AuthenticationError(str(e))
            
        return wrapper
    return decorator
```

## 貢獻指南

### 提交 Pull Request

1. **確保代碼品質（測試框架待建立）**
   ```bash
   # 手動驗證功能正常運作
   flake8 .
   mypy .
   ```

2. **更新文檔**
   - 更新相關的 API 文檔
   - 添加或修改使用示例
   - 更新 CHANGELOG.md

3. **填寫 PR 模板**
   ```markdown
   ## 變更摘要
   簡述這次 PR 的主要變更內容
   
   ## 變更類型
   - [ ] Bug 修復
   - [ ] 新功能
   - [ ] 文檔更新
   - [ ] 代碼重構
   - [ ] 性能優化
   
   ## 測試
   - [ ] 添加了相應的測試案例
   - [ ] 所有測試都通過
   - [ ] 手動測試通過
   
   ## 檢查清單
   - [ ] 代碼遵循項目編碼規範
   - [ ] 自我代碼審查完成
   - [ ] 添加了必要的註釋
   - [ ] 相關文檔已更新
   ```

### 代碼審查標準

1. **功能正確性**
   - 代碼邏輯是否正確
   - 是否處理了邊界情況
   - 錯誤處理是否完善

2. **代碼質量**
   - 是否遵循編碼規範
   - 代碼結構是否清晰
   - 是否有重複代碼

3. **性能考慮**
   - 是否有性能瓶頸
   - 資源使用是否合理
   - 是否可以優化

4. **安全性**
   - 輸入驗證是否充分
   - 是否有安全漏洞
   - 敏感信息是否保護

### 發布檢查清單

發布新版本前的檢查清單：

- [ ] 所有測試通過
- [ ] 文檔已更新
- [ ] CHANGELOG 已更新
- [ ] 版本號已更新
- [ ] 性能測試通過
- [ ] 安全掃描通過
- [ ] 向後兼容性檢查
- [ ] 部署腳本測試
- [ ] 監控和告警配置
- [ ] 回滾計畫準備

這份開發者指南涵蓋了 HiveMind 項目開發的各個方面。請根據實際需要參考相應章節，並隨著項目發展持續更新和完善。

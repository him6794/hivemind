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
│  │ │Rewards  │ │    │ │Rewards  │ │    │ │Reporter │ │      │
│  │ └─────────┘ │    │ └─────────┘ │    │ └─────────┘ │      │
│  └─────────────┘    └─────────────┘    └─────────────┘      │
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

### 項目結構

```
hivemind/
├── 📁 node_pool/                   # 節點池服務（核心組件）
│   ├── node_pool_server.py         # 主服務器入口（594行）
│   ├── user_service.py             # 用戶認證服務（292行）
│   ├── node_manager_service.py     # 節點管理服務（1714行）
│   ├── master_node_service.py      # 主控節點服務（1714行）
│   ├── database_manager.py         # 資料庫管理（125行）
│   ├── config.py                   # 配置管理（31行）
│   ├── nodepool_pb2.py            # gRPC 協議定義
│   └── nodepool_pb2_grpc.py       # gRPC 服務存根
│
├── 📁 master/                      # 主控節點
│   ├── master_node.py             # 主控節點主程序（1798行）
│   ├── vpn.py                     # VPN 管理功能（315行）
│   ├── nodepool_pb2.py           # 協議定義
│   └── templates_master/          # Web 模板
│
├── 📁 worker/                      # 工作節點
│   ├── worker_node.py             # 工作節點主程序（1798行）
│   ├── nodepool_pb2.py           # 協議定義
│   ├── nodepool.proto            # Protocol Buffers 定義
│   └── static/, templates/        # Web 資源
│
├── 📁 taskworker/                  # 任務工作庫
│   ├── worker.py                  # 任務執行器（292行）
│   ├── storage.py                 # 存儲管理（166行）
│   ├── dns_proxy.py              # DNS 代理（103行）
│   ├── rpc_service.py            # RPC 服務（61行）
│   └── protos/                   # gRPC 協議定義
│
├── 📁 ai/                          # AI 模組（開發中）
│   ├── main.py                   # AI 主程序（61行）
│   ├── breakdown.py              # 任務分割（47行）
│   └── Identification.py         # 身份識別（30行）
│
├── 📁 bt/                          # BT 模組
│   ├── tracker.py                # BitTorrent 追蹤器（134行）
│   ├── seeder.py                 # 種子服務器（88行）
│   └── create_torrent.py         # 種子創建工具（47行）
│
├── 📁 web/                         # Web 服務
│   ├── app.py                    # Flask Web 應用（205行）
│   ├── vpn_service.py           # VPN 服務（135行）
│   ├── wireguard_server.py      # WireGuard 服務器（92行）
│   └── static/, templates/       # Web 資源
│
└── 📁 documentation/               # 統一文檔中心
    ├── zh-tw/                    # 繁體中文文檔
    └── en/                       # 英文文檔
```

### 核心組件說明

#### 1. Node Pool (節點池)
- **作用**: 系統核心，負責節點管理、任務調度、用戶認證
- **主要文件**: `node_pool_server.py` (594行)
- **gRPC 服務**: 提供完整的節點管理 API
- **數據存儲**: SQLite (用戶) + Redis (狀態)

#### 2. Master Node (主控節點)
- **作用**: 提供管理界面、VPN 管理、系統監控
- **主要文件**: `master_node.py` (1798行)
- **功能**: Web 界面、VPN 配置、任務監控

#### 3. Worker Node (工作節點)
- **作用**: 執行實際計算任務、報告狀態
- **主要文件**: `worker_node.py` (1798行)
- **功能**: 任務執行、資源監控、結果回傳

#### 4. TaskWorker (任務庫)
- **作用**: 獨立的任務執行庫，可被其他項目整合
- **主要文件**: `worker.py` (292行)
- **特點**: 輕量級、可擴展、獨立部署

## 開發環境設置

### 1. 系統要求

- **Python**: 3.8+
- **Redis**: 6.0+
- **Git**: 2.20+
- **作業系統**: Linux/macOS/Windows

### 2. 克隆項目

```bash
git clone https://github.com/him6794/hivemind.git
cd hivemind
```

### 3. 設置虛擬環境

```bash
# 創建虛擬環境
python -m venv hivemind-env

# 啟動虛擬環境
# Linux/macOS:
source hivemind-env/bin/activate
# Windows:
hivemind-env\Scripts\activate
```

### 4. 安裝依賴

```bash
# 安裝全域依賴
pip install -r requirements.txt

# 安裝開發依賴
pip install pytest pytest-cov black flake8 mypy

# 安裝模組特定依賴
pip install -r master/requirements.txt
pip install -r worker/requirements.txt
pip install -r taskworker/requirements.txt
```

### 5. 設置 Redis

```bash
# Ubuntu/Debian
sudo apt update && sudo apt install redis-server

# macOS
brew install redis

# 啟動 Redis
redis-server
```

### 6. 配置環境變數

創建 `.env` 文件：

```bash
# Node Pool 配置
NODEPOOL_HOST=localhost
NODEPOOL_PORT=50051
REDIS_HOST=localhost
REDIS_PORT=6379

# 開發模式
DEBUG=true
LOG_LEVEL=DEBUG

# 測試數據庫
TEST_DB_PATH=test_users.db
```

## 代碼結構說明

### gRPC 協議定義

```protobuf
// nodepool.proto
service NodePoolService {
  // 節點管理
  rpc RegisterNode(RegisterNodeRequest) returns (RegisterNodeResponse);
  rpc UnregisterNode(UnregisterNodeRequest) returns (UnregisterNodeResponse);
  
  // 用戶管理
  rpc RegisterUser(RegisterUserRequest) returns (RegisterUserResponse);
  rpc LoginUser(LoginUserRequest) returns (LoginUserResponse);
  
  // 任務管理
  rpc SubmitTask(SubmitTaskRequest) returns (SubmitTaskResponse);
  rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
}
```

### 主要類結構

#### Node Pool Server
```python
class NodePoolServer:
    def __init__(self):
        self.redis_client = redis.Redis()
        self.user_service = UserService()
        self.node_manager = NodeManagerService()
        
    def serve(self):
        # 啟動 gRPC 服務器
        
class UserService:
    def RegisterUser(self, request, context):
        # 用戶註冊邏輯
        
class NodeManagerService:
    def RegisterNode(self, request, context):
        # 節點註冊邏輯
```

#### Worker Node
```python
class WorkerNode:
    def __init__(self, node_id):
        self.node_id = node_id
        self.executor = TaskExecutor()
        self.monitor = ResourceMonitor()
        
    def start(self):
        # 啟動工作節點
        
class TaskExecutor:
    def execute_task(self, task_data):
        # 執行任務邏輯
```

## 開發工作流

### 1. 分支策略

```bash
main           # 穩定版本
├── develop    # 開發主分支
├── feature/*  # 功能分支
├── hotfix/*   # 熱修復分支
└── release/*  # 發布分支
```

### 2. 功能開發流程

```bash
# 1. 創建功能分支
git checkout develop
git pull origin develop
git checkout -b feature/new-feature

# 2. 開發和測試
# ... 編寫代碼 ...
python -m pytest tests/

# 3. 代碼檢查
black .
flake8 .
mypy .

# 4. 提交代碼
git add .
git commit -m "feat: add new feature"

# 5. 推送並創建 PR
git push origin feature/new-feature
# 在 GitHub 上創建 Pull Request
```

### 3. 代碼規範

#### Python 代碼風格

```python
# 使用 Black 格式化
# 使用 type hints
def process_task(task_data: Dict[str, Any]) -> TaskResult:
    """處理任務數據
    
    Args:
        task_data: 任務輸入數據
        
    Returns:
        處理結果
        
    Raises:
        TaskExecutionError: 任務執行失敗時拋出
    """
    pass

# 使用 dataclass 定義數據結構
@dataclass
class TaskConfig:
    timeout: int = 300
    max_retries: int = 3
    priority: str = "normal"
```

#### 命名規範

- **文件名**: `snake_case.py`
- **類名**: `PascalCase`
- **函數/變數**: `snake_case`
- **常數**: `UPPER_SNAKE_CASE`
- **私有成員**: `_leading_underscore`

#### 文檔字符串

```python
def register_node(node_info: NodeInfo) -> bool:
    """註冊新的工作節點
    
    Args:
        node_info: 節點信息，包含ID、地址、能力等
        
    Returns:
        註冊成功返回 True，失敗返回 False
        
    Example:
        >>> node_info = NodeInfo(id="worker-1", host="192.168.1.100")
        >>> success = register_node(node_info)
        >>> print(success)
        True
    """
    pass
```

## 測試策略

### 1. 測試結構

```
tests/
├── unit/                    # 單元測試
│   ├── test_user_service.py
│   ├── test_node_manager.py
│   └── test_task_executor.py
├── integration/             # 整合測試
│   ├── test_grpc_services.py
│   └── test_worker_integration.py
├── e2e/                     # 端到端測試
│   └── test_full_workflow.py
└── fixtures/                # 測試固件
    ├── sample_tasks.json
    └── test_data.py
```

### 2. 測試範例

```python
# tests/unit/test_user_service.py
import pytest
from node_pool.user_service import UserService

class TestUserService:
    def setup_method(self):
        self.user_service = UserService(test_mode=True)
        
    def test_register_user_success(self):
        """測試用戶註冊成功"""
        request = RegisterUserRequest(
            username="testuser",
            password="password123",
            email="test@example.com"
        )
        response = self.user_service.RegisterUser(request, None)
        assert response.success is True
        assert response.user_id is not None
        
    def test_register_duplicate_user(self):
        """測試重複用戶註冊失敗"""
        # 第一次註冊
        request = RegisterUserRequest(...)
        self.user_service.RegisterUser(request, None)
        
        # 第二次註冊應該失敗
        response = self.user_service.RegisterUser(request, None)
        assert response.success is False
        assert "already exists" in response.message
```

### 3. 運行測試

```bash
# 運行所有測試
pytest

# 運行特定測試
pytest tests/unit/test_user_service.py

# 生成覆蓋率報告
pytest --cov=node_pool --cov-report=html

# 運行端到端測試
pytest tests/e2e/ -v
```

## 部署和 CI/CD

### 1. GitHub Actions 配置

```yaml
# .github/workflows/test.yml
name: Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    
    services:
      redis:
        image: redis:6.2
        ports:
          - 6379:6379
          
    steps:
    - uses: actions/checkout@v3
    - name: Set up Python
      uses: actions/setup-python@v3
      with:
        python-version: '3.9'
        
    - name: Install dependencies
      run: |
        pip install -r requirements.txt
        pip install pytest pytest-cov
        
    - name: Run tests
      run: pytest --cov=./ --cov-report=xml
      
    - name: Upload coverage
      uses: codecov/codecov-action@v3
```

### 2. Docker 化部署

```dockerfile
# Dockerfile
FROM python:3.9-slim

WORKDIR /app

COPY requirements.txt .
RUN pip install -r requirements.txt

COPY . .

EXPOSE 50051

CMD ["python", "node_pool/node_pool_server.py"]
```

```yaml
# docker-compose.yml
version: '3.8'

services:
  redis:
    image: redis:6.2-alpine
    ports:
      - "6379:6379"
      
  nodepool:
    build: .
    ports:
      - "50051:50051"
    depends_on:
      - redis
    environment:
      - REDIS_HOST=redis
      
  worker:
    build: 
      context: .
      dockerfile: worker/Dockerfile
    depends_on:
      - nodepool
    deploy:
      replicas: 3
```

## 擴展開發

### 1. 添加新的 gRPC 服務

```bash
# 1. 修改 proto 文件
vim nodepool.proto

# 2. 生成 Python 代碼
python -m grpc_tools.protoc \
    --python_out=. \
    --grpc_python_out=. \
    nodepool.proto

# 3. 實現服務邏輯
class NewService:
    def NewMethod(self, request, context):
        # 實現邏輯
        pass
```

### 2. 添加新的任務類型

```python
# taskworker/tasks/new_task.py
from taskworker.worker import BaseTask

class NewTaskType(BaseTask):
    def execute(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """執行新任務類型"""
        # 實現任務邏輯
        return {"result": "success"}
        
# 註冊任務類型
from taskworker.registry import register_task
register_task("new_task", NewTaskType)
```

### 3. 添加新的 Web API

```python
# web/api/new_endpoint.py
from flask import Blueprint, request, jsonify

new_api = Blueprint('new_api', __name__)

@new_api.route('/api/new-endpoint', methods=['POST'])
def handle_new_request():
    data = request.get_json()
    # 處理邏輯
    return jsonify({"status": "success"})
```

## 性能優化

### 1. gRPC 優化

```python
# 連接池配置
channel = grpc.insecure_channel(
    'localhost:50051',
    options=[
        ('grpc.keepalive_time_ms', 30000),
        ('grpc.keepalive_timeout_ms', 5000),
        ('grpc.http2.max_pings_without_data', 0),
        ('grpc.http2.min_ping_interval_without_data_ms', 300000),
    ]
)
```

### 2. Redis 優化

```python
# 使用連接池
import redis.connection
pool = redis.ConnectionPool(
    host='localhost',
    port=6379,
    db=0,
    max_connections=20
)
redis_client = redis.Redis(connection_pool=pool)

# 批量操作
pipe = redis_client.pipeline()
for key, value in data.items():
    pipe.hset(key, mapping=value)
pipe.execute()
```

### 3. 異步處理

```python
import asyncio
import aioredis

async def async_task_handler():
    redis = aioredis.from_url("redis://localhost")
    
    async def process_task(task_data):
        # 異步處理任務
        result = await some_async_operation(task_data)
        await redis.hset("results", task_id, result)
        
    tasks = [process_task(data) for data in task_list]
    await asyncio.gather(*tasks)
```

## 監控和日誌

### 1. 結構化日誌

```python
import structlog

logger = structlog.get_logger()

def process_task(task_id: str, user_id: str):
    logger.info(
        "Task processing started",
        task_id=task_id,
        user_id=user_id,
        module="task_processor"
    )
    
    try:
        result = execute_task()
        logger.info(
            "Task completed successfully",
            task_id=task_id,
            result_size=len(result)
        )
    except Exception as e:
        logger.error(
            "Task execution failed",
            task_id=task_id,
            error=str(e),
            exc_info=True
        )
```

### 2. 指標收集

```python
from prometheus_client import Counter, Histogram, Gauge

# 定義指標
task_counter = Counter('tasks_total', 'Total number of tasks')
task_duration = Histogram('task_duration_seconds', 'Task execution time')
active_workers = Gauge('active_workers', 'Number of active workers')

# 使用指標
@task_duration.time()
def execute_task():
    task_counter.inc()
    # 執行任務
    
def update_worker_count(count):
    active_workers.set(count)
```

## 安全考量

### 1. 認證和授權

```python
import jwt
from functools import wraps

def require_auth(f):
    @wraps(f)
    def decorated_function(*args, **kwargs):
        token = request.headers.get('Authorization')
        if not token:
            return jsonify({'error': 'No token provided'}), 401
            
        try:
            payload = jwt.decode(token, SECRET_KEY, algorithms=['HS256'])
            current_user = payload['user_id']
        except jwt.InvalidTokenError:
            return jsonify({'error': 'Invalid token'}), 401
            
        return f(current_user, *args, **kwargs)
    return decorated_function
```

### 2. 輸入驗證

```python
from marshmallow import Schema, fields, validate

class TaskSubmissionSchema(Schema):
    task_type = fields.Str(required=True, validate=validate.Length(min=1, max=50))
    task_data = fields.Raw(required=True)
    priority = fields.Str(validate=validate.OneOf(['low', 'normal', 'high']))

def submit_task():
    schema = TaskSubmissionSchema()
    try:
        result = schema.load(request.json)
    except ValidationError as err:
        return jsonify({'errors': err.messages}), 400
```

## 貢獻指南

### 1. 提交 Issue

使用以下模板報告 Bug：

```markdown
## Bug 描述
簡潔描述遇到的問題

## 重現步驟
1. 執行 `command`
2. 觀察結果

## 預期行為
描述您期望的正確行為

## 實際行為
描述實際發生的情況

## 環境信息
- OS: Ubuntu 20.04
- Python: 3.9.5
- HiveMind 版本: v1.0.0
```

### 2. 提交 Pull Request

```markdown
## 變更描述
描述此 PR 的目的和變更內容

## 變更類型
- [ ] Bug 修復
- [ ] 新功能
- [ ] 文檔更新
- [ ] 性能優化

## 測試清單
- [ ] 單元測試通過
- [ ] 整合測試通過
- [ ] 代碼覆蓋率 > 80%
- [ ] 文檔已更新

## 相關 Issue
Closes #123
```

### 3. 代碼審查清單

**功能性**：
- [ ] 代碼實現了預期功能
- [ ] 邊界條件處理正確
- [ ] 錯誤處理完善

**代碼質量**：
- [ ] 代碼風格符合規範
- [ ] 變數命名清晰
- [ ] 函數職責單一

**性能**：
- [ ] 無明顯性能問題
- [ ] 資源使用合理
- [ ] 無記憶體洩漏

**安全性**：
- [ ] 輸入驗證充分
- [ ] 無 SQL 注入風險
- [ ] 敏感信息妥善處理

## 常見問題

### Q: 如何調試 gRPC 服務？

```python
# 啟用 gRPC 調試日誌
import os
os.environ['GRPC_VERBOSITY'] = 'DEBUG'
os.environ['GRPC_TRACE'] = 'all'

# 使用 grpcurl 測試
grpcurl -plaintext localhost:50051 list
grpcurl -plaintext localhost:50051 describe NodePoolService
```

### Q: 如何模擬分布式環境測試？

```bash
# 使用 Docker Compose 創建多節點環境
docker-compose up --scale worker=5

# 或使用 Kind 創建 Kubernetes 測試環境
kind create cluster
kubectl apply -f k8s/
```

### Q: 如何進行性能測試？

```python
# 使用 locust 進行負載測試
from locust import User, task, between
import grpc

class HiveMindUser(User):
    wait_time = between(1, 3)
    
    def on_start(self):
        self.channel = grpc.insecure_channel('localhost:50051')
        self.stub = nodepool_pb2_grpc.NodePoolServiceStub(self.channel)
    
    @task
    def submit_task(self):
        request = nodepool_pb2.SubmitTaskRequest(...)
        response = self.stub.SubmitTask(request)
```

---

**更新日期**: 2024年1月  
**版本**: v1.0  
**狀態**: 完整的開發者指南

**如需幫助**: 請參考項目 [Wiki](https://github.com/him6794/hivemind/wiki) 或在 [Discord](https://discord.gg/hivemind) 中聯繫開發團隊。

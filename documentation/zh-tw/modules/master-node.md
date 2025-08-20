# Master Node 模組文檔

## 📋 概述

Master Node 是 HiveMind 分散式計算平台的管理中心，提供用戶界面、任務管理和系統監控功能。它作為整個系統的控制平面，協調各個組件的工作。

## 🏗️ 系統架構

```
┌─────────────────────┐
│    Master Node      │
├─────────────────────┤
│ • Web Interface     │
│ • Task Manager      │
│ • HiveMind Integ    │
│ • VPN Management    │
│ • User Dashboard    │
└─────────────────────┘
        │
        ├─ Flask Web Server (Port 5000)
        ├─ Node Pool gRPC Client
        └─ WireGuard VPN
```

## 🔧 核心組件

### 1. Master Node Server (`master_node.py`)
- **功能**: 主要 Flask Web 應用服務器
- **端口**: 5000
- **協議**: HTTP/HTTPS Web Interface

**主要功能**:
```python
# Web 路由
@app.route('/')
def dashboard()

@app.route('/login', methods=['GET', 'POST'])
def login()

@app.route('/upload', methods=['GET', 'POST'])
def upload_task()

@app.route('/tasks')
def view_tasks()

@app.route('/nodes')
def view_nodes()
```

### 2. HiveMind Integration (`hivemind_integration.py`)
- **功能**: 與 Node Pool 的 gRPC 通信接口
- **職責**:
  - 任務提交和管理
  - 節點狀態查詢
  - 用戶身份驗證中繼

### 3. VPN Management (`vpn.py`)
- **功能**: WireGuard VPN 配置管理
- **職責**:
  - VPN 配置文件生成
  - 客戶端連接管理
  - 網路安全隧道

### 4. Task Splitter (`task_splitter.py`)
- **功能**: 大型任務分解和分配
- **職責**:
  - 任務分片算法
  - 負載均衡策略
  - 結果聚合

## 🌐 Web 界面功能

### 用戶認證系統
- **登入頁面**: 用戶身份驗證
- **會話管理**: Session 和 Cookie 處理
- **權限控制**: 基於角色的訪問控制

### 任務管理界面
- **任務上傳**: 文件上傳和任務配置
- **任務監控**: 實時狀態和進度顯示
- **結果下載**: 完成任務的結果獲取

### 節點監控面板
- **節點狀態**: 活躍節點列表和狀態
- **性能指標**: CPU、記憶體、網路使用率
- **歷史數據**: 節點性能歷史圖表

### 系統儀表板
- **系統概覽**: 整體系統健康狀態
- **統計資料**: 任務執行統計和趨勢
- **告警通知**: 系統異常和警告信息

## 📡 與 Node Pool 通信

### gRPC 客戶端實現
```python
import grpc
from master import nodepool_pb2, nodepool_pb2_grpc

class NodePoolClient:
    def __init__(self, node_pool_address='localhost:50051'):
        self.channel = grpc.insecure_channel(node_pool_address)
        self.node_stub = nodepool_pb2_grpc.NodeManagerStub(self.channel)
        self.user_stub = nodepool_pb2_grpc.UserManagerStub(self.channel)
        self.master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(self.channel)
    
    def submit_task(self, task_data):
        request = nodepool_pb2.SubmitTaskRequest(
            task_type=task_data['type'],
            task_data=task_data['data'],
            priority=task_data.get('priority', 1)
        )
        return self.master_stub.SubmitTask(request)
    
    def get_node_list(self):
        request = nodepool_pb2.GetNodeListRequest()
        return self.node_stub.GetNodeList(request)
```

## 🗂️ 文件結構

```
master/
├── master_node.py              # 主要 Flask 應用
├── hivemind_integration.py     # Node Pool 集成
├── vpn.py                      # VPN 管理
├── task_splitter.py           # 任務分解器
├── nodepool_pb2.py            # Protocol Buffer 生成文件
├── nodepool_pb2_grpc.py       # gRPC 生成文件
├── requirements.txt           # Python 依賴包
├── file.ico                   # 應用圖標
├── templates_master/          # Jinja2 模板
│   ├── login.html            # 登入頁面
│   ├── master_dashboard.html # 主儀表板
│   └── master_upload.html    # 任務上傳頁面
└── HiveMind-master-Release/   # 發布包
    ├── install.sh            # 安裝腳本
    ├── start_hivemind.cmd    # Windows 啟動腳本
    └── ...
```

## 🚀 部署和配置

### 本地開發環境
```bash
cd master
pip install -r requirements.txt
python master_node.py
```

### 生產環境部署
```bash
# 使用 Gunicorn
gunicorn -w 4 -b 0.0.0.0:5000 master_node:app

# 使用 systemd 服務
sudo systemctl enable hivemind-master
sudo systemctl start hivemind-master
```

### 配置文件範例
```python
# config.py
import os

class Config:
    SECRET_KEY = os.environ.get('SECRET_KEY') or 'dev-secret-key'
    NODE_POOL_ADDRESS = os.environ.get('NODE_POOL_ADDRESS') or 'localhost:50051'
    UPLOAD_FOLDER = os.environ.get('UPLOAD_FOLDER') or './uploads'
    MAX_CONTENT_LENGTH = 16 * 1024 * 1024  # 16MB
    
    # VPN 配置
    VPN_SERVER_IP = os.environ.get('VPN_SERVER_IP') or '10.0.0.1'
    VPN_PORT = int(os.environ.get('VPN_PORT') or 51820)
    
    # 數據庫配置
    DATABASE_URL = os.environ.get('DATABASE_URL') or 'sqlite:///master.db'
```

## 🔐 VPN 管理

### WireGuard 配置生成
```python
def generate_vpn_config(user_id, client_ip):
    """為用戶生成 WireGuard VPN 配置"""
    private_key = generate_private_key()
    public_key = private_key.public_key()
    
    config = f"""
[Interface]
PrivateKey = {private_key}
Address = {client_ip}/24
DNS = 8.8.8.8

[Peer]
PublicKey = {SERVER_PUBLIC_KEY}
Endpoint = {VPN_SERVER_IP}:{VPN_PORT}
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
    return config
```

### VPN 客戶端管理
- **配置分發**: 自動生成客戶端配置文件
- **連接監控**: 客戶端連接狀態追蹤
- **訪問控制**: 基於用戶權限的網路訪問

## 📊 任務管理系統

### 任務生命週期
```python
class TaskStatus:
    PENDING = "pending"
    ASSIGNED = "assigned"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"

class TaskManager:
    def submit_task(self, task_data):
        """提交新任務到系統"""
        task_id = self.generate_task_id()
        
        # 任務分解
        subtasks = self.split_task(task_data)
        
        # 提交到 Node Pool
        for subtask in subtasks:
            self.submit_subtask(task_id, subtask)
        
        return task_id
    
    def monitor_task(self, task_id):
        """監控任務執行狀態"""
        subtasks = self.get_subtasks(task_id)
        
        total = len(subtasks)
        completed = sum(1 for st in subtasks if st.status == TaskStatus.COMPLETED)
        
        return {
            'progress': (completed / total) * 100,
            'status': self.calculate_overall_status(subtasks)
        }
```

### 任務分解策略
```python
def split_task(self, task_data):
    """根據任務類型分解任務"""
    if task_data['type'] == 'data_processing':
        return self.split_data_processing_task(task_data)
    elif task_data['type'] == 'machine_learning':
        return self.split_ml_task(task_data)
    elif task_data['type'] == 'simulation':
        return self.split_simulation_task(task_data)
    else:
        return [task_data]  # 不分解
```

## 🎨 前端界面

### HTML 模板結構
```html
<!-- templates_master/master_dashboard.html -->
<!DOCTYPE html>
<html>
<head>
    <title>HiveMind Master Dashboard</title>
    <link rel="stylesheet" href="/static/css/dashboard.css">
</head>
<body>
    <nav class="navbar">
        <div class="nav-brand">HiveMind Master</div>
        <div class="nav-links">
            <a href="/dashboard">儀表板</a>
            <a href="/tasks">任務管理</a>
            <a href="/nodes">節點監控</a>
            <a href="/vpn">VPN 管理</a>
        </div>
    </nav>
    
    <main class="dashboard">
        <!-- 儀表板內容 -->
    </main>
    
    <script src="/static/js/dashboard.js"></script>
</body>
</html>
```

### JavaScript 功能
```javascript
// static/js/dashboard.js
class Dashboard {
    constructor() {
        this.updateInterval = 5000; // 5秒更新一次
        this.init();
    }
    
    init() {
        this.updateNodeStatus();
        this.updateTaskStatus();
        setInterval(() => this.updateAll(), this.updateInterval);
    }
    
    async updateNodeStatus() {
        const response = await fetch('/api/nodes/status');
        const nodes = await response.json();
        this.renderNodeStatus(nodes);
    }
    
    async updateTaskStatus() {
        const response = await fetch('/api/tasks/status');
        const tasks = await response.json();
        this.renderTaskStatus(tasks);
    }
}

// 初始化儀表板
document.addEventListener('DOMContentLoaded', () => {
    new Dashboard();
});
```

## 🔍 監控和日誌

### 系統指標收集
```python
import psutil
import time

class SystemMonitor:
    def get_system_metrics(self):
        return {
            'cpu_percent': psutil.cpu_percent(interval=1),
            'memory': psutil.virtual_memory()._asdict(),
            'disk': psutil.disk_usage('/')._asdict(),
            'network': psutil.net_io_counters()._asdict(),
            'timestamp': time.time()
        }
    
    def get_process_metrics(self):
        process = psutil.Process()
        return {
            'cpu_percent': process.cpu_percent(),
            'memory_info': process.memory_info()._asdict(),
            'num_threads': process.num_threads(),
            'connections': len(process.connections())
        }
```

### 日誌配置
```python
import logging
from logging.handlers import RotatingFileHandler

def setup_logging(app):
    if not app.debug:
        file_handler = RotatingFileHandler(
            'logs/master_node.log', 
            maxBytes=10240000, 
            backupCount=10
        )
        file_handler.setFormatter(logging.Formatter(
            '%(asctime)s %(levelname)s: %(message)s [in %(pathname)s:%(lineno)d]'
        ))
        file_handler.setLevel(logging.INFO)
        app.logger.addHandler(file_handler)
        app.logger.setLevel(logging.INFO)
        app.logger.info('Master Node startup')
```

## 🛠️ API 端點

### RESTful API
```python
# API 路由
@app.route('/api/tasks', methods=['GET'])
def api_get_tasks():
    """獲取任務列表"""
    return jsonify({'tasks': task_manager.get_all_tasks()})

@app.route('/api/tasks', methods=['POST'])
def api_submit_task():
    """提交新任務"""
    task_data = request.json
    task_id = task_manager.submit_task(task_data)
    return jsonify({'task_id': task_id})

@app.route('/api/nodes', methods=['GET'])
def api_get_nodes():
    """獲取節點列表"""
    nodes = hivemind_client.get_node_list()
    return jsonify({'nodes': nodes})

@app.route('/api/system/status', methods=['GET'])
def api_system_status():
    """獲取系統狀態"""
    return jsonify(system_monitor.get_system_metrics())
```

## 🔧 常見問題排除

### 1. Flask 應用啟動失敗
**問題**: `Address already in use`
**解決**:
```bash
# 查找占用 5000 端口的進程
lsof -i :5000

# 終止進程
kill -9 <PID>
```

### 2. Node Pool 連接失敗
**問題**: gRPC 連接超時
**解決**:
```python
# 增加連接重試機制
import time
from grpc import RpcError

def connect_to_node_pool(max_retries=3):
    for attempt in range(max_retries):
        try:
            channel = grpc.insecure_channel('localhost:50051')
            stub = nodepool_pb2_grpc.NodeManagerStub(channel)
            # 測試連接
            stub.GetNodeList(nodepool_pb2.GetNodeListRequest())
            return stub
        except RpcError:
            if attempt < max_retries - 1:
                time.sleep(2 ** attempt)  # 指數退避
            else:
                raise
```

### 3. VPN 配置問題
**問題**: WireGuard 連接失敗
**解決**:
```bash
# 檢查 WireGuard 服務狀態
sudo systemctl status wg-quick@wg0

# 檢查防火牆設置
sudo ufw allow 51820/udp

# 檢查路由表
ip route show
```

## 📈 性能優化

### Flask 應用優化
```python
# 使用 Redis 作為 Session 存儲
from flask_session import Session
import redis

app.config['SESSION_TYPE'] = 'redis'
app.config['SESSION_REDIS'] = redis.from_url('redis://localhost:6379')
Session(app)

# 啟用 gzip 壓縮
from flask_compress import Compress
Compress(app)

# 靜態文件快取
@app.after_request
def after_request(response):
    if request.endpoint == 'static':
        response.cache_control.max_age = 86400  # 1天
    return response
```

### 前端優化
- **資源壓縮**: JavaScript 和 CSS 文件壓縮
- **快取策略**: 瀏覽器快取和 CDN 配置
- **非同步載入**: Ajax 請求和頁面非同步更新

## 🔄 維護和更新

### 自動部署腳本
```bash
#!/bin/bash
# deploy_master.sh

# 停止現有服務
sudo systemctl stop hivemind-master

# 更新代碼
git pull origin main

# 安裝依賴
pip install -r requirements.txt

# 數據庫遷移（如果需要）
python manage.py db upgrade

# 重啟服務
sudo systemctl start hivemind-master

# 檢查狀態
sudo systemctl status hivemind-master
```

### 健康檢查
```python
@app.route('/health')
def health_check():
    """系統健康檢查端點"""
    try:
        # 檢查 Node Pool 連接
        hivemind_client.ping()
        
        # 檢查數據庫連接
        db.session.execute('SELECT 1')
        
        return jsonify({
            'status': 'healthy',
            'timestamp': time.time(),
            'version': app.config['VERSION']
        })
    except Exception as e:
        return jsonify({
            'status': 'unhealthy',
            'error': str(e),
            'timestamp': time.time()
        }), 500
```

---

**相關文檔**:
- [Node Pool 模組](node-pool.md)
- [Worker Node 模組](worker-node.md)
- [API 文檔](../api.md)
- [部署指南](../deployment.md)

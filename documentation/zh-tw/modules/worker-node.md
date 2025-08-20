# Worker Node 工作節點模組文檔

HiveMind Worker Node 是分散式計算平台的核心執行單元，提供了企業級的任務執行、資源管理和監控能力。

## 📋 概述

Worker Node 是一個完整的分散式計算執行節點，具備以下特性：

### 核心特色
- **🎯 多任務並行執行**：支援同時執行多個計算任務
- **🐳 Docker 容器化執行**：安全隔離的任務執行環境
- **🔗 自動 VPN 連接**：自動連接到 HiveMind 分散式網路
- **📊 即時資源監控**：CPU、記憶體、GPU 即時監控
- **🌐 Web 管理介面**：現代化的節點管理界面
- **⚡ 信任評分系統**：基於節點表現的動態信任評分
- **🔒 用戶會話管理**：安全的多用戶會話支援

### 技術架構

```
┌─────────────────────────────────────────────────────┐
│                Worker Node Architecture             │
├─────────────────────────────────────────────────────┤
│                                                     │
│  ┌─────────────────┐    ┌─────────────────┐        │
│  │   Flask Web     │    │   gRPC Service  │        │
│  │   Interface     │    │   (Port 50053)  │        │
│  │  (Port 5000)    │    │                 │        │
│  └─────────────────┘    └─────────────────┘        │
│           │                       │                 │
│           └───────────┬───────────┘                 │
│                       │                             │
│  ┌─────────────────────────────────────────────────┐│
│  │            Core WorkerNode Class                ││
│  │  ┌─────────────────────────────────────────────┐││
│  │  │ Multi-Task Execution Engine                 │││
│  │  │ • running_tasks: Dict[task_id, task_info]   │││
│  │  │ • task_locks: Dict[task_id, Lock]           │││
│  │  │ • task_stop_events: Dict[task_id, Event]    │││
│  │  └─────────────────────────────────────────────┘││
│  │  ┌─────────────────────────────────────────────┐││
│  │  │ Resource Management System                  │││
│  │  │ • available_resources: CPU/Memory/GPU       │││
│  │  │ • total_resources: System capabilities      │││
│  │  │ • resources_lock: Thread-safe access       │││
│  │  └─────────────────────────────────────────────┘││
│  │  ┌─────────────────────────────────────────────┐││
│  │  │ Trust & Security System                     │││
│  │  │ • trust_score: 0-100 reliability score     │││
│  │  │ • trust_group: high/medium/low              │││
│  │  │ • user_sessions: Multi-user support        │││
│  │  └─────────────────────────────────────────────┘││
│  └─────────────────────────────────────────────────┘│
│           │                       │                 │
│  ┌─────────────────┐    ┌─────────────────┐        │
│  │  Docker Engine  │    │ VPN Connection  │        │
│  │                 │    │ WireGuard Auto  │        │
│  └─────────────────┘    └─────────────────┘        │
└─────────────────────────────────────────────────────┘
```

## 🗂️ 新架構檔案結構

```
worker/
├── src/
│   └── hivemind_worker/           # 主要 Python 套件
│       ├── worker_node.py         # 核心工作節點實現
│       ├── nodepool_pb2.py        # gRPC Protocol Buffers
│       ├── nodepool_pb2_grpc.py   # gRPC 服務客戶端
│       ├── __init__.py            # 套件初始化
│       ├── static/                # Web 界面靜態資源
│       │   ├── css/              # 樣式文件
│       │   ├── js/               # JavaScript 文件
│       │   └── images/           # 圖片資源
│       └── templates/             # Flask HTML 模板
│           ├── dashboard.html     # 主儀表板
│           ├── login.html         # 登入頁面
│           └── tasks.html         # 任務管理頁面
├── main.py                        # 主入口點
├── pyproject.toml                 # Python 專案配置
├── requirements.txt               # 依賴套件
├── Dockerfile                     # Docker 建構文件
├── run_task.sh                    # 任務執行腳本
├── nodepool.proto                 # gRPC 服務定義
└── README.md                      # 說明文件
```

## 🔧 核心組件詳解

### 1. 主要 WorkerNode 類別

```python
class WorkerNode:
    def __init__(self):
        # 節點基本信息
        self.node_id = NODE_ID
        self.port = NODE_PORT
        self.master_address = MASTER_ADDRESS
        
        # 多任務執行系統
        self.running_tasks = {}     # {task_id: task_info}
        self.task_locks = {}        # {task_id: threading.Lock()}
        self.task_stop_events = {}  # {task_id: Event()}
        
        # 資源管理系統
        self.available_resources = {
            "cpu": 0,               # CPU 分數
            "memory_gb": 0,         # 可用記憶體 GB
            "gpu": 0,               # GPU 分數  
            "gpu_memory_gb": 0      # GPU 記憶體 GB
        }
        
        # 信任與安全系統
        self.trust_score = 0        # 0-100 信任分數
        self.trust_group = "low"    # high/medium/low
        self.user_sessions = {}     # 多用戶會話
```

### 2. 多任務執行引擎

**並行任務支援**：
```python
def execute_task(self, task_id, task_data):
    """執行任務（支援並行）"""
    # 1. 創建任務專用鎖和停止事件
    self.task_locks[task_id] = threading.Lock()
    self.task_stop_events[task_id] = threading.Event()
    
    # 2. 分配資源
    required_resources = self._calculate_task_resources(task_data)
    if not self._allocate_resources(task_id, required_resources):
        return {"error": "Insufficient resources"}
    
    # 3. 在獨立線程中執行任務
    task_thread = threading.Thread(
        target=self._run_task_in_thread,
        args=(task_id, task_data),
        name=f"Task-{task_id}"
    )
    task_thread.start()
    
    return {"status": "started", "task_id": task_id}

def _run_task_in_thread(self, task_id, task_data):
    """在獨立線程中執行任務"""
    try:
        # 更新任務狀態為運行中
        with self.task_locks[task_id]:
            self.running_tasks[task_id] = {
                "status": "RUNNING",
                "start_time": time(),
                "resources": task_data.get("required_resources", {})
            }
        
        # 執行 Docker 容器任務
        result = self._execute_docker_task(task_id, task_data)
        
        # 更新完成狀態
        with self.task_locks[task_id]:
            self.running_tasks[task_id]["status"] = "COMPLETED"
            self.running_tasks[task_id]["result"] = result
            
    except Exception as e:
        with self.task_locks[task_id]:
            self.running_tasks[task_id]["status"] = "FAILED"
            self.running_tasks[task_id]["error"] = str(e)
    finally:
        # 釋放資源
        self._release_task_resources(task_id)
```

### 3. 動態資源管理

**智能資源分配**：
```python
def _allocate_resources(self, task_id, required_resources):
    """為任務分配資源"""
    with self.resources_lock:
        # 檢查是否有足夠資源
        if (self.available_resources["cpu"] >= required_resources.get("cpu", 0) and
            self.available_resources["memory_gb"] >= required_resources.get("memory_gb", 0) and
            self.available_resources["gpu"] >= required_resources.get("gpu", 0)):
            
            # 分配資源
            for resource, amount in required_resources.items():
                self.available_resources[resource] -= amount
            
            # 記錄任務資源使用
            self.running_tasks[task_id] = {
                "status": "ALLOCATED",
                "resources": required_resources,
                "start_time": time()
            }
            return True
        
        return False

def _release_task_resources(self, task_id):
    """釋放任務資源"""
    with self.resources_lock:
        if task_id in self.running_tasks:
            task_resources = self.running_tasks[task_id].get("resources", {})
            
            # 歸還資源
            for resource, amount in task_resources.items():
                if resource in self.available_resources:
                    self.available_resources[resource] += amount
                    # 確保不超過總資源限制
                    self.available_resources[resource] = min(
                        self.available_resources[resource],
                        self.total_resources[resource]
                    )
```

### 4. VPN 自動連接系統

**自動網路加入**：
```python
def _auto_join_vpn(self):
    """自動連接到 HiveMind VPN 網路"""
    try:
        # 檢查是否已連接到 VPN
        if self._check_vpn_connection():
            self._log("Already connected to HiveMind network")
            return
        
        # 引導用戶手動連接（非交互模式）
        self._log("VPN connection required for HiveMind network")
        self._log("Please ensure WireGuard VPN is configured and running")
        
        # 等待用戶確認連接
        print("如果您已經連接，請按 y")
        response = input()
        if response.lower() == 'y':
            self._log("VPN connection confirmed")
        
    except Exception as e:
        self._log(f"VPN setup failed: {e}", WARNING)

def _get_local_ip(self):
    """獲取本機 IP（優先 WireGuard 網卡）"""
    try:
        interfaces_list = interfaces()
        
        # 優先檢查 WireGuard 網卡
        wg_interfaces = [iface for iface in interfaces_list 
                        if 'wg' in iface.lower() or 'wireguard' in iface.lower()]
        
        if wg_interfaces:
            for wg_iface in wg_interfaces:
                addrs = ifaddresses(wg_iface)
                if AF_INET in addrs:
                    return addrs[AF_INET][0]['addr']
        
        # 檢查 10.0.0.x VPN 網段
        for iface in interfaces_list:
            addrs = ifaddresses(iface)
            if AF_INET in addrs:
                for addr_info in addrs[AF_INET]:
                    ip = addr_info['addr']
                    if ip.startswith('10.0.0.') and ip != '10.0.0.1':
                        return ip
        
        return '127.0.0.1'
        
    except Exception as e:
        self._log(f"IP detection failed: {e}")
        return '127.0.0.1'
```

## 🌐 Web 管理介面

Worker Node 提供了現代化的 Web 管理介面，支援：

### 主要功能頁面

1. **儀表板 (`/dashboard`)**
   - 節點即時狀態監控
   - 資源使用率圖表
   - 任務執行統計
   - 系統效能指標

2. **任務管理 (`/tasks`)**
   - 當前運行任務列表
   - 任務歷史記錄
   - 任務詳細信息
   - 任務停止控制

3. **系統監控 (`/monitor`)**
   - CPU 使用率
   - 記憶體使用狀況
   - Docker 狀態
   - 網路連接狀態

### RESTful API 端點

```python
# 系統狀態 API
@app.route('/api/status')
def api_status():
    """獲取節點完整狀態信息"""
    return jsonify({
        'node_id': self.node_id,
        'status': self.status,
        'is_registered': self.is_registered,
        'docker_available': self.docker_available,
        'cpu_percent': cpu_percent(),
        'memory_percent': virtual_memory().percent,
        'available_resources': self.available_resources,
        'total_resources': self.total_resources,
        'running_tasks': len(self.running_tasks),
        'trust_score': self.trust_score,
        'trust_group': self.trust_group
    })

# 任務管理 API
@app.route('/api/tasks')
def api_tasks():
    """獲取所有運行中的任務"""
    tasks_info = []
    with self.resources_lock:
        for task_id, task_data in self.running_tasks.items():
            tasks_info.append({
                'task_id': task_id,
                'status': task_data.get('status', 'Unknown'),
                'start_time': datetime.fromtimestamp(
                    task_data.get('start_time', 0)
                ).isoformat(),
                'elapsed': time() - task_data.get('start_time', time()),
                'resources': task_data.get('resources', {})
            })
    return jsonify({'tasks': tasks_info})

# 任務控制 API
@app.route('/api/stop_task/<task_id>', methods=['POST'])
def api_stop_task(task_id):
    """停止指定任務"""
    if task_id in self.task_stop_events:
        self.task_stop_events[task_id].set()
        return jsonify({'success': True, 'message': f'Task {task_id} stopped'})
    return jsonify({'success': False, 'error': 'Task not found'}), 404
```

## 🐳 Docker 容器管理

### 容器化任務執行

```python
def _execute_docker_task(self, task_id, task_data):
    """在 Docker 容器中執行任務"""
    try:
        # 1. 準備工作目錄
        work_dir = self._prepare_task_workspace(task_id, task_data)
        
        # 2. 創建 Docker 容器
        container = self.docker_client.containers.run(
            image=task_data.get('docker_image', 'justin308/hivemind-worker:latest'),
            command=task_data.get('command', 'python task.py'),
            name=f"task-{task_id}",
            volumes={work_dir: {'bind': '/workspace', 'mode': 'rw'}},
            working_dir='/workspace',
            mem_limit=task_data.get('memory_limit', '1g'),
            cpu_quota=task_data.get('cpu_quota', 100000),
            detach=True,
            remove=False,
            network_mode='bridge',
            environment=task_data.get('environment', {})
        )
        
        # 3. 監控容器執行
        while container.status != 'exited':
            # 檢查停止信號
            if self.task_stop_events[task_id].is_set():
                container.stop(timeout=10)
                break
                
            sleep(1)
            container.reload()
        
        # 4. 收集執行結果
        logs = container.logs(stdout=True, stderr=True).decode('utf-8')
        exit_code = container.attrs['State']['ExitCode']
        
        # 5. 收集輸出文件
        result_files = self._collect_task_results(container, work_dir)
        
        # 6. 清理容器
        container.remove()
        
        return {
            'exit_code': exit_code,
            'logs': logs,
            'files': result_files,
            'status': 'completed' if exit_code == 0 else 'failed'
        }
        
    except Exception as e:
        self._log(f"Docker task execution failed: {e}", ERROR)
        return {'error': str(e), 'status': 'failed'}
```

## 📊 信任評分系統

Worker Node 實現了動態信任評分機制：

### 信任分數計算

```python
def update_trust_score(self, task_result):
    """更新節點信任分數"""
    if task_result.get('status') == 'completed':
        # 成功完成任務提升信任分數
        self.trust_score = min(100, self.trust_score + 2)
    elif task_result.get('status') == 'failed':
        # 任務失敗降低信任分數
        self.trust_score = max(0, self.trust_score - 5)
    
    # 更新信任分組
    if self.trust_score >= 80:
        self.trust_group = "high"
    elif self.trust_score >= 50:
        self.trust_group = "medium"
    else:
        self.trust_group = "low"
    
    self._log(f"Trust score updated: {self.trust_score} (Group: {self.trust_group})")
```

## 🚀 部署和配置

### 環境變數配置

```bash
# 基本配置
NODE_PORT=50053                    # gRPC 服務端口
FLASK_PORT=5000                    # Web 界面端口
MASTER_ADDRESS=10.0.0.1:50051     # Master Node 地址
NODE_ID=worker-hostname-50053      # 節點唯一識別

# Docker 配置
DOCKER_ENABLED=true                # 啟用 Docker 支援
DOCKER_NETWORK=hivemind           # Docker 網路名稱

# 資源配置
MAX_CPU_CORES=4                   # 最大 CPU 核心數
MAX_MEMORY_GB=8                   # 最大記憶體 GB
MAX_CONCURRENT_TASKS=3            # 最大並行任務數

# 安全配置
SESSION_TIMEOUT=24                # 會話超時時間（小時）
TRUST_THRESHOLD=50               # 信任分數閾值
```

### 使用 Python 套件安裝

```bash
# 1. 從原始碼安裝
cd worker/
pip install -e .

# 2. 使用預建套件
pip install hivemind_worker==0.0.7

# 3. 執行 Worker Node
python -m hivemind_worker.worker_node
```

### Docker 部署

```bash
# 1. 建構 Docker 映像
docker build -t hivemind-worker:latest .

# 2. 執行容器
docker run -d \
  --name hivemind-worker \
  --network host \
  -e MASTER_ADDRESS=10.0.0.1:50051 \
  -e NODE_PORT=50053 \
  -e FLASK_PORT=5000 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  hivemind-worker:latest
```

## 🔍 監控和日誌

### 日誌系統

Worker Node 提供完整的日誌記錄：

```python
def _log(self, message, level=INFO):
    """線程安全的日誌記錄"""
    with self.log_lock:
        timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        log_entry = f"[{timestamp}] {getLevelName(level)}: {message}"
        
        # 添加到內存日誌
        self.logs.append({
            'timestamp': timestamp,
            'level': getLevelName(level),
            'message': message
        })
        
        # 保持日誌大小限制
        if len(self.logs) > 1000:
            self.logs = self.logs[-500:]  # 保留最新 500 條
        
        print(log_entry)
```

### 健康檢查

```python
@app.route('/health')
def health_check():
    """健康檢查端點"""
    checks = {
        'docker': self._check_docker_health(),
        'grpc': self._check_grpc_connection(),
        'resources': self._check_resource_availability(),
        'vpn': self._check_vpn_connection()
    }
    
    all_healthy = all(check['status'] == 'healthy' for check in checks.values())
    
    return jsonify({
        'status': 'healthy' if all_healthy else 'unhealthy',
        'checks': checks,
        'timestamp': datetime.now().isoformat()
    }), 200 if all_healthy else 503
```

## 🔧 常見問題排解

### 1. Docker 連接問題

```bash
# 檢查 Docker 服務狀態
systemctl status docker

# 重啟 Docker 服務
sudo systemctl restart docker

# 檢查 Docker 權限
sudo usermod -aG docker $USER
```

### 2. VPN 連接問題

```bash
# 檢查 WireGuard 狀態
sudo wg show

# 重啟 WireGuard
sudo systemctl restart wg-quick@wg0

# 檢查路由表
ip route | grep 10.0.0
```

### 3. 資源不足問題

```python
# 檢查系統資源
@app.route('/api/resources')
def check_resources():
    return jsonify({
        'cpu_available': psutil.cpu_percent(interval=1),
        'memory_available': psutil.virtual_memory().available / (1024**3),
        'disk_available': psutil.disk_usage('/').free / (1024**3),
        'running_tasks': len(self.running_tasks)
    })
```

### 4. 任務執行失敗

檢查任務日誌：
```bash
# 檢查容器日誌
docker logs task-{task_id}

# 檢查工作目錄
ls -la /tmp/hivemind/tasks/{task_id}/
```

## 📈 效能優化

### 1. 資源調度優化

```python
def _optimize_resource_allocation(self):
    """優化資源分配策略"""
    # 根據歷史數據調整資源分配
    cpu_utilization = self._get_average_cpu_usage()
    memory_utilization = self._get_average_memory_usage()
    
    # 動態調整可用資源
    if cpu_utilization < 0.7:
        self.available_resources['cpu'] = min(
            self.total_resources['cpu'],
            self.available_resources['cpu'] * 1.1
        )
```

### 2. 網路優化

```python
def _optimize_network_settings(self):
    """優化網路設定"""
    # 調整 gRPC 連接參數
    options = [
        ('grpc.keepalive_time_ms', 30000),
        ('grpc.keepalive_timeout_ms', 10000),
        ('grpc.keepalive_permit_without_calls', True),
        ('grpc.http2.max_pings_without_data', 0),
        ('grpc.http2.min_time_between_pings_ms', 10000),
        ('grpc.http2.min_ping_interval_without_data_ms', 300000)
    ]
    return options
```

這個更新的 Worker Node 文檔反映了最新的架構改進，包括多任務支援、信任評分系統、VPN 自動連接和現代化的 Web 介面。

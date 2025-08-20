# Worker Node 模組文檔

## 📋 概述

Worker Node 是 HiveMind 分散式計算平台的計算執行節點，負責接收和執行來自 Master Node 分配的計算任務，並將結果回傳給系統。

## 🏗️ 系統架構

```
┌─────────────────────┐
│    Worker Node      │
├─────────────────────┤
│ • Task Executor     │
│ • Resource Monitor  │
│ • Result Handler    │
│ • Status Reporter   │
│ • Docker Engine     │
└─────────────────────┘
        │
        ├─ gRPC Client (to Node Pool)
        ├─ Docker Containers
        ├─ Local Storage
        └─ System Resources
```

## 🔧 核心組件

### 1. Worker Node Main (`worker_node.py`)
- **功能**: 主要工作節點服務
- **協議**: gRPC Client to Node Pool
- **容器**: Docker 基礎任務執行環境

**主要功能**:
```python
class WorkerNode:
    def __init__(self, node_id, node_pool_address):
        self.node_id = node_id
        self.node_pool_client = NodePoolClient(node_pool_address)
        self.task_executor = TaskExecutor()
        self.resource_monitor = ResourceMonitor()
        
    def start(self):
        """啟動工作節點"""
        self.register_with_node_pool()
        self.start_heartbeat()
        self.start_task_polling()
        
    def register_with_node_pool(self):
        """向 Node Pool 註冊節點"""
        
    def execute_task(self, task):
        """執行分配的任務"""
        
    def report_result(self, task_id, result):
        """回報任務執行結果"""
```

### 2. Task Executor (`task_executor.py`)
- **功能**: 任務執行引擎
- **支援**: Docker 容器化執行
- **隔離**: 進程和資源隔離

### 3. Resource Monitor (`resource_monitor.py`)
- **功能**: 系統資源監控
- **指標**: CPU、記憶體、磁碟、網路
- **報告**: 實時資源使用情況

### 4. Docker Manager (`docker_manager.py`)
- **功能**: Docker 容器管理
- **職責**: 容器創建、執行、清理
- **安全**: 容器安全配置

## 🗂️ 文件結構

```
worker/
├── worker_node.py              # 主要工作節點服務
├── task_executor.py           # 任務執行引擎
├── resource_monitor.py        # 資源監控器
├── docker_manager.py          # Docker 容器管理
├── nodepool_pb2.py           # Protocol Buffer 文件
├── nodepool_pb2_grpc.py      # gRPC 客戶端文件
├── nodepool.proto            # Protocol Buffer 定義
├── requirements.txt          # Python 依賴包
├── Dockerfile               # Docker 鏡像構建文件
├── run_task.sh             # 任務執行腳本
├── build.py                # 構建腳本
├── make.py                 # 編譯腳本
├── file.ico                # 應用圖標
├── README.md               # 模組說明文檔
├── README.en.md            # 英文說明文檔
├── static/                 # 靜態資源
├── templates/              # HTML 模板
├── hivemind_worker/        # Worker 打包項目
└── HiveMind-Worker-Release/ # 發布包
    ├── install.sh          # 安裝腳本
    ├── start_worker.cmd    # Windows 啟動腳本
    └── ...
```

## 🚀 部署和配置

### 本地開發環境
```bash
cd worker
pip install -r requirements.txt
python worker_node.py --node-id=worker-001 --node-pool=localhost:50051
```

### Docker 容器部署
```bash
# 構建 Docker 鏡像
docker build -t hivemind-worker .

# 運行 Worker 容器
docker run -d \
  --name hivemind-worker-001 \
  -e NODE_ID=worker-001 \
  -e NODE_POOL_ADDRESS=host.docker.internal:50051 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  hivemind-worker
```

### 配置文件範例
```python
# config.py
import os

class WorkerConfig:
    # 節點配置
    NODE_ID = os.environ.get('NODE_ID') or f'worker-{uuid.uuid4().hex[:8]}'
    NODE_POOL_ADDRESS = os.environ.get('NODE_POOL_ADDRESS') or 'localhost:50051'
    
    # 資源限制
    MAX_CPU_CORES = int(os.environ.get('MAX_CPU_CORES') or 0)  # 0 = 無限制
    MAX_MEMORY_GB = float(os.environ.get('MAX_MEMORY_GB') or 0)  # 0 = 無限制
    MAX_DISK_GB = float(os.environ.get('MAX_DISK_GB') or 0)    # 0 = 無限制
    
    # 任務配置
    TASK_TIMEOUT = int(os.environ.get('TASK_TIMEOUT') or 3600)  # 1小時
    MAX_CONCURRENT_TASKS = int(os.environ.get('MAX_CONCURRENT_TASKS') or 1)
    
    # 心跳配置
    HEARTBEAT_INTERVAL = int(os.environ.get('HEARTBEAT_INTERVAL') or 30)  # 30秒
    
    # Docker 配置
    DOCKER_ENABLED = os.environ.get('DOCKER_ENABLED', 'true').lower() == 'true'
    DOCKER_NETWORK = os.environ.get('DOCKER_NETWORK') or 'hivemind'
    
    # 存儲配置
    WORK_DIR = os.environ.get('WORK_DIR') or './work'
    TEMP_DIR = os.environ.get('TEMP_DIR') or './temp'
    LOG_DIR = os.environ.get('LOG_DIR') or './logs'
```

## 📡 與 Node Pool 通信

### gRPC 客戶端實現
```python
import grpc
import time
from concurrent import futures
from worker import nodepool_pb2, nodepool_pb2_grpc

class NodePoolClient:
    def __init__(self, address):
        self.address = address
        self.channel = grpc.insecure_channel(address)
        self.stub = nodepool_pb2_grpc.NodeManagerStub(self.channel)
        
    def register_node(self, node_info):
        """註冊節點到 Node Pool"""
        request = nodepool_pb2.RegisterNodeRequest(
            node_id=node_info['node_id'],
            ip_address=node_info['ip_address'],
            port=node_info['port'],
            cpu_cores=node_info['cpu_cores'],
            memory_gb=node_info['memory_gb'],
            disk_gb=node_info['disk_gb'],
            capabilities=node_info.get('capabilities', [])
        )
        
        try:
            response = self.stub.RegisterNode(request)
            return response.success
        except grpc.RpcError as e:
            print(f"註冊節點失敗: {e}")
            return False
    
    def send_heartbeat(self, node_id, status):
        """發送心跳信號"""
        request = nodepool_pb2.UpdateNodeStatusRequest(
            node_id=node_id,
            status=status,
            timestamp=int(time.time())
        )
        
        try:
            response = self.stub.UpdateNodeStatus(request)
            return response.success
        except grpc.RpcError as e:
            print(f"心跳發送失敗: {e}")
            return False
    
    def get_assigned_tasks(self, node_id):
        """獲取分配給此節點的任務"""
        request = nodepool_pb2.GetAssignedTasksRequest(node_id=node_id)
        
        try:
            response = self.stub.GetAssignedTasks(request)
            return response.tasks
        except grpc.RpcError as e:
            print(f"獲取任務失敗: {e}")
            return []
    
    def report_task_result(self, task_id, result, status):
        """回報任務執行結果"""
        request = nodepool_pb2.ReportTaskResultRequest(
            task_id=task_id,
            node_id=self.node_id,
            result=result,
            status=status,
            timestamp=int(time.time())
        )
        
        try:
            response = self.stub.ReportTaskResult(request)
            return response.success
        except grpc.RpcError as e:
            print(f"結果回報失敗: {e}")
            return False
```

## 🐳 Docker 任務執行

### Docker 容器管理
```python
import docker
import json
import tempfile
import os

class DockerTaskExecutor:
    def __init__(self):
        self.client = docker.from_env()
        self.network_name = 'hivemind'
        self.ensure_network_exists()
    
    def ensure_network_exists(self):
        """確保 HiveMind 網路存在"""
        try:
            self.client.networks.get(self.network_name)
        except docker.errors.NotFound:
            self.client.networks.create(
                self.network_name,
                driver="bridge",
                options={"com.docker.network.bridge.enable_icc": "true"}
            )
    
    def execute_task(self, task):
        """在 Docker 容器中執行任務"""
        container_name = f"hivemind-task-{task['id']}"
        
        # 準備任務數據
        work_dir = tempfile.mkdtemp(prefix=f"task-{task['id']}-")
        self.prepare_task_data(task, work_dir)
        
        try:
            # 創建並啟動容器
            container = self.client.containers.run(
                image=task.get('docker_image', 'python:3.9-slim'),
                command=task['command'],
                name=container_name,
                network=self.network_name,
                volumes={
                    work_dir: {'bind': '/workspace', 'mode': 'rw'}
                },
                working_dir='/workspace',
                mem_limit=task.get('memory_limit', '1g'),
                cpu_quota=task.get('cpu_quota', 100000),  # 1 CPU core
                detach=True,
                remove=False  # 保留容器以獲取結果
            )
            
            # 等待容器完成
            result = container.wait(timeout=task.get('timeout', 3600))
            
            # 獲取輸出
            logs = container.logs(stdout=True, stderr=True).decode('utf-8')
            
            # 獲取結果文件
            result_data = self.collect_results(container, work_dir)
            
            # 清理容器
            container.remove()
            
            return {
                'status': 'completed' if result['StatusCode'] == 0 else 'failed',
                'exit_code': result['StatusCode'],
                'logs': logs,
                'result_data': result_data
            }
            
        except docker.errors.ContainerError as e:
            return {
                'status': 'failed',
                'error': str(e),
                'exit_code': e.exit_status
            }
        except Exception as e:
            return {
                'status': 'failed',
                'error': str(e)
            }
        finally:
            # 清理工作目錄
            self.cleanup_work_dir(work_dir)
    
    def prepare_task_data(self, task, work_dir):
        """準備任務執行所需的數據文件"""
        # 寫入任務配置
        with open(os.path.join(work_dir, 'task.json'), 'w') as f:
            json.dump(task, f, indent=2)
        
        # 寫入任務數據文件
        if 'input_files' in task:
            for filename, content in task['input_files'].items():
                file_path = os.path.join(work_dir, filename)
                os.makedirs(os.path.dirname(file_path), exist_ok=True)
                
                if isinstance(content, str):
                    with open(file_path, 'w') as f:
                        f.write(content)
                else:
                    with open(file_path, 'wb') as f:
                        f.write(content)
    
    def collect_results(self, container, work_dir):
        """收集任務執行結果"""
        result_data = {}
        
        # 從容器複製結果文件
        try:
            # 獲取結果目錄內容
            archive, _ = container.get_archive('/workspace/results')
            
            # 解壓縮並讀取文件
            import tarfile
            import io
            
            tar = tarfile.open(fileobj=io.BytesIO(archive.read()))
            for member in tar.getmembers():
                if member.isfile():
                    file_data = tar.extractfile(member).read()
                    result_data[member.name] = file_data
            
        except docker.errors.NotFound:
            # 沒有結果目錄，檢查是否有結果文件
            pass
        
        return result_data
```

### 任務類型支援

#### Python 計算任務
```python
def execute_python_task(self, task):
    """執行 Python 計算任務"""
    python_code = task['code']
    requirements = task.get('requirements', [])
    
    # 創建 Dockerfile
    dockerfile_content = f"""
FROM python:3.9-slim

# 安裝依賴包
RUN pip install {' '.join(requirements)}

# 複製任務代碼
COPY task.py /app/task.py
WORKDIR /app

# 執行任務
CMD ["python", "task.py"]
"""
    
    return self.execute_custom_docker_task(task, dockerfile_content, {'task.py': python_code})
```

#### 機器學習任務
```python
def execute_ml_task(self, task):
    """執行機器學習任務"""
    model_type = task['model_type']
    training_data = task['training_data']
    
    if model_type == 'tensorflow':
        return self.execute_tensorflow_task(task)
    elif model_type == 'pytorch':
        return self.execute_pytorch_task(task)
    elif model_type == 'scikit-learn':
        return self.execute_sklearn_task(task)
    else:
        raise ValueError(f"不支援的模型類型: {model_type}")
```

#### 數據處理任務
```python
def execute_data_processing_task(self, task):
    """執行數據處理任務"""
    processing_type = task['processing_type']
    input_data = task['input_data']
    
    if processing_type == 'csv_analysis':
        return self.execute_csv_analysis(task)
    elif processing_type == 'image_processing':
        return self.execute_image_processing(task)
    elif processing_type == 'text_analysis':
        return self.execute_text_analysis(task)
    else:
        raise ValueError(f"不支援的處理類型: {processing_type}")
```

## 📊 資源監控

### 系統資源監控
```python
import psutil
import time
import threading

class ResourceMonitor:
    def __init__(self, update_interval=5):
        self.update_interval = update_interval
        self.monitoring = False
        self.metrics = {}
        
    def start_monitoring(self):
        """開始資源監控"""
        self.monitoring = True
        monitor_thread = threading.Thread(target=self._monitor_loop)
        monitor_thread.daemon = True
        monitor_thread.start()
    
    def stop_monitoring(self):
        """停止資源監控"""
        self.monitoring = False
    
    def _monitor_loop(self):
        """監控主循環"""
        while self.monitoring:
            self.metrics = self.collect_metrics()
            time.sleep(self.update_interval)
    
    def collect_metrics(self):
        """收集系統指標"""
        return {
            'timestamp': time.time(),
            'cpu': {
                'percent': psutil.cpu_percent(interval=1),
                'count': psutil.cpu_count(),
                'freq': psutil.cpu_freq()._asdict() if psutil.cpu_freq() else None
            },
            'memory': psutil.virtual_memory()._asdict(),
            'disk': psutil.disk_usage('/')._asdict(),
            'network': psutil.net_io_counters()._asdict(),
            'processes': len(psutil.pids()),
            'load_avg': os.getloadavg() if hasattr(os, 'getloadavg') else None
        }
    
    def get_current_metrics(self):
        """獲取當前指標"""
        return self.metrics.copy()
    
    def is_resource_available(self, required_resources):
        """檢查是否有足夠的資源執行任務"""
        current = self.get_current_metrics()
        
        # 檢查 CPU
        if 'cpu_cores' in required_resources:
            if current['cpu']['percent'] > 90:  # CPU 使用率過高
                return False
        
        # 檢查記憶體
        if 'memory_gb' in required_resources:
            required_memory = required_resources['memory_gb'] * 1024 * 1024 * 1024
            available_memory = current['memory']['available']
            if required_memory > available_memory:
                return False
        
        # 檢查磁碟空間
        if 'disk_gb' in required_resources:
            required_disk = required_resources['disk_gb'] * 1024 * 1024 * 1024
            available_disk = current['disk']['free']
            if required_disk > available_disk:
                return False
        
        return True
```

### 容器資源監控
```python
class ContainerMonitor:
    def __init__(self, docker_client):
        self.docker_client = docker_client
        
    def monitor_container(self, container_id):
        """監控指定容器的資源使用情況"""
        try:
            container = self.docker_client.containers.get(container_id)
            stats = container.stats(stream=False)
            
            return self.parse_container_stats(stats)
        except docker.errors.NotFound:
            return None
    
    def parse_container_stats(self, stats):
        """解析容器統計數據"""
        # CPU 使用率計算
        cpu_delta = stats['cpu_stats']['cpu_usage']['total_usage'] - \
                   stats['precpu_stats']['cpu_usage']['total_usage']
        system_delta = stats['cpu_stats']['system_cpu_usage'] - \
                      stats['precpu_stats']['system_cpu_usage']
        
        cpu_percent = 0.0
        if system_delta > 0:
            cpu_percent = (cpu_delta / system_delta) * len(stats['cpu_stats']['cpu_usage']['percpu_usage']) * 100.0
        
        # 記憶體使用情況
        memory_usage = stats['memory_stats']['usage']
        memory_limit = stats['memory_stats']['limit']
        memory_percent = (memory_usage / memory_limit) * 100.0
        
        # 網路 I/O
        network_rx = 0
        network_tx = 0
        for interface, data in stats['networks'].items():
            network_rx += data['rx_bytes']
            network_tx += data['tx_bytes']
        
        return {
            'cpu_percent': cpu_percent,
            'memory_usage': memory_usage,
            'memory_limit': memory_limit,
            'memory_percent': memory_percent,
            'network_rx': network_rx,
            'network_tx': network_tx,
            'timestamp': time.time()
        }
```

## 🔍 日誌和監控

### 結構化日誌
```python
import logging
import json
from datetime import datetime

class StructuredLogger:
    def __init__(self, name, log_file=None):
        self.logger = logging.getLogger(name)
        self.logger.setLevel(logging.INFO)
        
        # 創建格式化器
        formatter = logging.Formatter(
            '%(asctime)s - %(name)s - %(levelname)s - %(message)s'
        )
        
        # 控制台處理器
        console_handler = logging.StreamHandler()
        console_handler.setFormatter(formatter)
        self.logger.addHandler(console_handler)
        
        # 文件處理器
        if log_file:
            file_handler = logging.FileHandler(log_file)
            file_handler.setFormatter(formatter)
            self.logger.addHandler(file_handler)
    
    def log_task_event(self, event_type, task_id, details=None):
        """記錄任務相關事件"""
        log_data = {
            'timestamp': datetime.utcnow().isoformat(),
            'event_type': event_type,
            'task_id': task_id,
            'node_id': self.node_id,
            'details': details or {}
        }
        
        self.logger.info(json.dumps(log_data))
    
    def log_system_event(self, event_type, details=None):
        """記錄系統相關事件"""
        log_data = {
            'timestamp': datetime.utcnow().isoformat(),
            'event_type': event_type,
            'node_id': self.node_id,
            'details': details or {}
        }
        
        self.logger.info(json.dumps(log_data))
```

### 健康檢查
```python
class HealthChecker:
    def __init__(self, worker_node):
        self.worker_node = worker_node
        
    def check_health(self):
        """執行健康檢查"""
        health_status = {
            'status': 'healthy',
            'timestamp': time.time(),
            'checks': {}
        }
        
        # 檢查 Node Pool 連接
        health_status['checks']['node_pool'] = self._check_node_pool_connection()
        
        # 檢查 Docker 服務
        health_status['checks']['docker'] = self._check_docker_service()
        
        # 檢查系統資源
        health_status['checks']['resources'] = self._check_system_resources()
        
        # 檢查磁碟空間
        health_status['checks']['disk_space'] = self._check_disk_space()
        
        # 判斷整體狀態
        if any(check['status'] == 'unhealthy' for check in health_status['checks'].values()):
            health_status['status'] = 'unhealthy'
        
        return health_status
    
    def _check_node_pool_connection(self):
        """檢查與 Node Pool 的連接"""
        try:
            self.worker_node.node_pool_client.ping()
            return {'status': 'healthy', 'message': 'Node Pool connection OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Node Pool connection failed: {str(e)}'}
    
    def _check_docker_service(self):
        """檢查 Docker 服務"""
        try:
            docker_client = docker.from_env()
            docker_client.ping()
            return {'status': 'healthy', 'message': 'Docker service OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Docker service failed: {str(e)}'}
    
    def _check_system_resources(self):
        """檢查系統資源"""
        try:
            cpu_percent = psutil.cpu_percent(interval=1)
            memory = psutil.virtual_memory()
            
            if cpu_percent > 95:
                return {'status': 'unhealthy', 'message': f'High CPU usage: {cpu_percent}%'}
            
            if memory.percent > 95:
                return {'status': 'unhealthy', 'message': f'High memory usage: {memory.percent}%'}
            
            return {'status': 'healthy', 'message': 'System resources OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Resource check failed: {str(e)}'}
    
    def _check_disk_space(self):
        """檢查磁碟空間"""
        try:
            disk = psutil.disk_usage('/')
            if disk.percent > 90:
                return {'status': 'unhealthy', 'message': f'Low disk space: {disk.percent}% used'}
            
            return {'status': 'healthy', 'message': 'Disk space OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Disk check failed: {str(e)}'}
```

## 🔧 常見問題排除

### 1. Docker 容器執行失敗
**問題**: 容器啟動或執行失敗
**解決**:
```bash
# 檢查 Docker 服務狀態
sudo systemctl status docker

# 檢查 Docker 映像是否存在
docker images

# 檢查容器日誌
docker logs <container_id>

# 清理停止的容器
docker container prune
```

### 2. Node Pool 連接問題
**問題**: 無法連接到 Node Pool 服務
**解決**:
```python
# 實施連接重試機制
import time
import grpc

def connect_with_retry(address, max_retries=5, retry_delay=2):
    for attempt in range(max_retries):
        try:
            channel = grpc.insecure_channel(address)
            # 測試連接
            grpc.channel_ready_future(channel).result(timeout=10)
            return channel
        except grpc.FutureTimeoutError:
            if attempt < max_retries - 1:
                print(f"連接失敗，{retry_delay}秒後重試...")
                time.sleep(retry_delay)
                retry_delay *= 2  # 指數退避
            else:
                raise Exception("無法連接到 Node Pool")
```

### 3. 資源不足問題
**問題**: 系統資源不足無法執行任務
**解決**:
```python
# 實施資源檢查和等待機制
def wait_for_resources(required_resources, timeout=300):
    start_time = time.time()
    
    while time.time() - start_time < timeout:
        if resource_monitor.is_resource_available(required_resources):
            return True
        
        print("資源不足，等待中...")
        time.sleep(10)
    
    return False

# 使用方式
if wait_for_resources({'memory_gb': 2, 'cpu_cores': 1}):
    execute_task(task)
else:
    report_task_failed(task_id, "資源不足")
```

## 📈 性能優化

### 任務執行優化
```python
# 任務預載入
class TaskPreloader:
    def __init__(self, docker_client):
        self.docker_client = docker_client
        self.preloaded_images = set()
    
    def preload_image(self, image_name):
        """預載入 Docker 映像"""
        if image_name not in self.preloaded_images:
            try:
                self.docker_client.images.pull(image_name)
                self.preloaded_images.add(image_name)
                print(f"預載入映像: {image_name}")
            except Exception as e:
                print(f"預載入映像失敗: {e}")

# 並行任務執行
from concurrent.futures import ThreadPoolExecutor, as_completed

class ParallelTaskExecutor:
    def __init__(self, max_concurrent_tasks=2):
        self.max_concurrent_tasks = max_concurrent_tasks
        self.executor = ThreadPoolExecutor(max_workers=max_concurrent_tasks)
        self.running_tasks = {}
    
    def submit_task(self, task):
        """提交任務進行並行執行"""
        if len(self.running_tasks) < self.max_concurrent_tasks:
            future = self.executor.submit(self.execute_task, task)
            self.running_tasks[task['id']] = future
            return True
        return False
    
    def check_completed_tasks(self):
        """檢查已完成的任務"""
        completed_tasks = []
        
        for task_id, future in list(self.running_tasks.items()):
            if future.done():
                try:
                    result = future.result()
                    completed_tasks.append({'task_id': task_id, 'result': result})
                except Exception as e:
                    completed_tasks.append({'task_id': task_id, 'error': str(e)})
                
                del self.running_tasks[task_id]
        
        return completed_tasks
```

### 資源使用優化
```python
# 記憶體管理
import gc

class MemoryManager:
    def __init__(self, memory_threshold=80):
        self.memory_threshold = memory_threshold
    
    def check_memory_usage(self):
        """檢查記憶體使用情況"""
        memory = psutil.virtual_memory()
        return memory.percent
    
    def cleanup_memory(self):
        """清理記憶體"""
        gc.collect()
        
        # 清理 Docker 未使用的映像
        docker_client = docker.from_env()
        docker_client.images.prune()
        
        # 清理未使用的容器
        docker_client.containers.prune()

# 磁碟空間管理
class DiskManager:
    def __init__(self, cleanup_threshold=85):
        self.cleanup_threshold = cleanup_threshold
    
    def cleanup_old_files(self, directory, days=7):
        """清理舊文件"""
        import os
        import time
        
        current_time = time.time()
        cutoff_time = current_time - (days * 24 * 60 * 60)
        
        for root, dirs, files in os.walk(directory):
            for file in files:
                file_path = os.path.join(root, file)
                if os.path.getmtime(file_path) < cutoff_time:
                    try:
                        os.remove(file_path)
                        print(f"刪除舊文件: {file_path}")
                    except Exception as e:
                        print(f"刪除文件失敗: {e}")
```

---

**相關文檔**:
- [Node Pool 模組](node-pool.md)
- [Master Node 模組](master-node.md)
- [TaskWorker 模組](taskworker.md)
- [API 文檔](../api.md)
- [部署指南](../deployment.md)

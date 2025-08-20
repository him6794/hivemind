# Worker Node Module Documentation

## 📋 Overview

The Worker Node is the computational execution node of the HiveMind distributed computing platform, responsible for receiving and executing computational tasks assigned by the Master Node and returning results to the system.

## 🏗️ System Architecture

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

## 🔧 Core Components

### 1. Worker Node Main (`worker_node.py`)
- **Function**: Main worker node service
- **Protocol**: gRPC Client to Node Pool
- **Container**: Docker-based task execution environment

**Main Features**:
```python
class WorkerNode:
    def __init__(self, node_id, node_pool_address):
        self.node_id = node_id
        self.node_pool_client = NodePoolClient(node_pool_address)
        self.task_executor = TaskExecutor()
        self.resource_monitor = ResourceMonitor()
        
    def start(self):
        """Start worker node"""
        self.register_with_node_pool()
        self.start_heartbeat()
        self.start_task_polling()
        
    def register_with_node_pool(self):
        """Register node with Node Pool"""
        
    def execute_task(self, task):
        """Execute assigned task"""
        
    def report_result(self, task_id, result):
        """Report task execution result"""
```

### 2. Task Executor (`task_executor.py`)
- **Function**: Task execution engine
- **Support**: Docker containerized execution
- **Isolation**: Process and resource isolation

### 3. Resource Monitor (`resource_monitor.py`)
- **Function**: System resource monitoring
- **Metrics**: CPU, memory, disk, network
- **Reporting**: Real-time resource usage

### 4. Docker Manager (`docker_manager.py`)
- **Function**: Docker container management
- **Responsibilities**: Container creation, execution, cleanup
- **Security**: Container security configuration

## 🗂️ File Structure

```
worker/
├── worker_node.py              # Main worker node service
├── task_executor.py           # Task execution engine
├── resource_monitor.py        # Resource monitor
├── docker_manager.py          # Docker container manager
├── nodepool_pb2.py           # Protocol Buffer files
├── nodepool_pb2_grpc.py      # gRPC client files
├── nodepool.proto            # Protocol Buffer definitions
├── requirements.txt          # Python dependencies
├── Dockerfile               # Docker image build file
├── run_task.sh             # Task execution script
├── build.py                # Build script
├── make.py                 # Compilation script
├── file.ico                # Application icon
├── README.md               # Module documentation
├── README.en.md            # English documentation
├── static/                 # Static resources
├── templates/              # HTML templates
├── hivemind_worker/        # Worker packaging project
└── HiveMind-Worker-Release/ # Release package
    ├── install.sh          # Installation script
    ├── start_worker.cmd    # Windows startup script
    └── ...
```

## 🚀 Deployment and Configuration

### Local Development Environment
```bash
cd worker
pip install -r requirements.txt
python worker_node.py --node-id=worker-001 --node-pool=localhost:50051
```

### Docker Container Deployment
```bash
# Build Docker image
docker build -t hivemind-worker .

# Run Worker container
docker run -d \
  --name hivemind-worker-001 \
  -e NODE_ID=worker-001 \
  -e NODE_POOL_ADDRESS=host.docker.internal:50051 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  hivemind-worker
```

### Configuration File Example
```python
# config.py
import os

class WorkerConfig:
    # Node configuration
    NODE_ID = os.environ.get('NODE_ID') or f'worker-{uuid.uuid4().hex[:8]}'
    NODE_POOL_ADDRESS = os.environ.get('NODE_POOL_ADDRESS') or 'localhost:50051'
    
    # Resource limits
    MAX_CPU_CORES = int(os.environ.get('MAX_CPU_CORES') or 0)  # 0 = unlimited
    MAX_MEMORY_GB = float(os.environ.get('MAX_MEMORY_GB') or 0)  # 0 = unlimited
    MAX_DISK_GB = float(os.environ.get('MAX_DISK_GB') or 0)    # 0 = unlimited
    
    # Task configuration
    TASK_TIMEOUT = int(os.environ.get('TASK_TIMEOUT') or 3600)  # 1 hour
    MAX_CONCURRENT_TASKS = int(os.environ.get('MAX_CONCURRENT_TASKS') or 1)
    
    # Heartbeat configuration
    HEARTBEAT_INTERVAL = int(os.environ.get('HEARTBEAT_INTERVAL') or 30)  # 30 seconds
    
    # Docker configuration
    DOCKER_ENABLED = os.environ.get('DOCKER_ENABLED', 'true').lower() == 'true'
    DOCKER_NETWORK = os.environ.get('DOCKER_NETWORK') or 'hivemind'
    
    # Storage configuration
    WORK_DIR = os.environ.get('WORK_DIR') or './work'
    TEMP_DIR = os.environ.get('TEMP_DIR') or './temp'
    LOG_DIR = os.environ.get('LOG_DIR') or './logs'
```

## 📡 Communication with Node Pool

### gRPC Client Implementation
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
        """Register node with Node Pool"""
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
            print(f"Node registration failed: {e}")
            return False
    
    def send_heartbeat(self, node_id, status):
        """Send heartbeat signal"""
        request = nodepool_pb2.UpdateNodeStatusRequest(
            node_id=node_id,
            status=status,
            timestamp=int(time.time())
        )
        
        try:
            response = self.stub.UpdateNodeStatus(request)
            return response.success
        except grpc.RpcError as e:
            print(f"Heartbeat send failed: {e}")
            return False
    
    def get_assigned_tasks(self, node_id):
        """Get tasks assigned to this node"""
        request = nodepool_pb2.GetAssignedTasksRequest(node_id=node_id)
        
        try:
            response = self.stub.GetAssignedTasks(request)
            return response.tasks
        except grpc.RpcError as e:
            print(f"Get tasks failed: {e}")
            return []
    
    def report_task_result(self, task_id, result, status):
        """Report task execution result"""
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
            print(f"Result report failed: {e}")
            return False
```

## 🐳 Docker Task Execution

### Docker Container Management
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
        """Ensure HiveMind network exists"""
        try:
            self.client.networks.get(self.network_name)
        except docker.errors.NotFound:
            self.client.networks.create(
                self.network_name,
                driver="bridge",
                options={"com.docker.network.bridge.enable_icc": "true"}
            )
    
    def execute_task(self, task):
        """Execute task in Docker container"""
        container_name = f"hivemind-task-{task['id']}"
        
        # Prepare task data
        work_dir = tempfile.mkdtemp(prefix=f"task-{task['id']}-")
        self.prepare_task_data(task, work_dir)
        
        try:
            # Create and start container
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
                remove=False  # Keep container to get results
            )
            
            # Wait for container completion
            result = container.wait(timeout=task.get('timeout', 3600))
            
            # Get output
            logs = container.logs(stdout=True, stderr=True).decode('utf-8')
            
            # Get result files
            result_data = self.collect_results(container, work_dir)
            
            # Cleanup container
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
            # Cleanup work directory
            self.cleanup_work_dir(work_dir)
    
    def prepare_task_data(self, task, work_dir):
        """Prepare task execution data files"""
        # Write task configuration
        with open(os.path.join(work_dir, 'task.json'), 'w') as f:
            json.dump(task, f, indent=2)
        
        # Write task data files
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
        """Collect task execution results"""
        result_data = {}
        
        # Copy result files from container
        try:
            # Get result directory contents
            archive, _ = container.get_archive('/workspace/results')
            
            # Extract and read files
            import tarfile
            import io
            
            tar = tarfile.open(fileobj=io.BytesIO(archive.read()))
            for member in tar.getmembers():
                if member.isfile():
                    file_data = tar.extractfile(member).read()
                    result_data[member.name] = file_data
            
        except docker.errors.NotFound:
            # No result directory, check for result files
            pass
        
        return result_data
```

### Task Type Support

#### Python Computation Tasks
```python
def execute_python_task(self, task):
    """Execute Python computation task"""
    python_code = task['code']
    requirements = task.get('requirements', [])
    
    # Create Dockerfile
    dockerfile_content = f"""
FROM python:3.9-slim

# Install dependencies
RUN pip install {' '.join(requirements)}

# Copy task code
COPY task.py /app/task.py
WORKDIR /app

# Execute task
CMD ["python", "task.py"]
"""
    
    return self.execute_custom_docker_task(task, dockerfile_content, {'task.py': python_code})
```

#### Machine Learning Tasks
```python
def execute_ml_task(self, task):
    """Execute machine learning task"""
    model_type = task['model_type']
    training_data = task['training_data']
    
    if model_type == 'tensorflow':
        return self.execute_tensorflow_task(task)
    elif model_type == 'pytorch':
        return self.execute_pytorch_task(task)
    elif model_type == 'scikit-learn':
        return self.execute_sklearn_task(task)
    else:
        raise ValueError(f"Unsupported model type: {model_type}")
```

#### Data Processing Tasks
```python
def execute_data_processing_task(self, task):
    """Execute data processing task"""
    processing_type = task['processing_type']
    input_data = task['input_data']
    
    if processing_type == 'csv_analysis':
        return self.execute_csv_analysis(task)
    elif processing_type == 'image_processing':
        return self.execute_image_processing(task)
    elif processing_type == 'text_analysis':
        return self.execute_text_analysis(task)
    else:
        raise ValueError(f"Unsupported processing type: {processing_type}")
```

## 📊 Resource Monitoring

### System Resource Monitoring
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
        """Start resource monitoring"""
        self.monitoring = True
        monitor_thread = threading.Thread(target=self._monitor_loop)
        monitor_thread.daemon = True
        monitor_thread.start()
    
    def stop_monitoring(self):
        """Stop resource monitoring"""
        self.monitoring = False
    
    def _monitor_loop(self):
        """Main monitoring loop"""
        while self.monitoring:
            self.metrics = self.collect_metrics()
            time.sleep(self.update_interval)
    
    def collect_metrics(self):
        """Collect system metrics"""
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
        """Get current metrics"""
        return self.metrics.copy()
    
    def is_resource_available(self, required_resources):
        """Check if sufficient resources available for task execution"""
        current = self.get_current_metrics()
        
        # Check CPU
        if 'cpu_cores' in required_resources:
            if current['cpu']['percent'] > 90:  # CPU usage too high
                return False
        
        # Check memory
        if 'memory_gb' in required_resources:
            required_memory = required_resources['memory_gb'] * 1024 * 1024 * 1024
            available_memory = current['memory']['available']
            if required_memory > available_memory:
                return False
        
        # Check disk space
        if 'disk_gb' in required_resources:
            required_disk = required_resources['disk_gb'] * 1024 * 1024 * 1024
            available_disk = current['disk']['free']
            if required_disk > available_disk:
                return False
        
        return True
```

### Container Resource Monitoring
```python
class ContainerMonitor:
    def __init__(self, docker_client):
        self.docker_client = docker_client
        
    def monitor_container(self, container_id):
        """Monitor resource usage of specified container"""
        try:
            container = self.docker_client.containers.get(container_id)
            stats = container.stats(stream=False)
            
            return self.parse_container_stats(stats)
        except docker.errors.NotFound:
            return None
    
    def parse_container_stats(self, stats):
        """Parse container statistics data"""
        # CPU usage calculation
        cpu_delta = stats['cpu_stats']['cpu_usage']['total_usage'] - \
                   stats['precpu_stats']['cpu_usage']['total_usage']
        system_delta = stats['cpu_stats']['system_cpu_usage'] - \
                      stats['precpu_stats']['system_cpu_usage']
        
        cpu_percent = 0.0
        if system_delta > 0:
            cpu_percent = (cpu_delta / system_delta) * len(stats['cpu_stats']['cpu_usage']['percpu_usage']) * 100.0
        
        # Memory usage
        memory_usage = stats['memory_stats']['usage']
        memory_limit = stats['memory_stats']['limit']
        memory_percent = (memory_usage / memory_limit) * 100.0
        
        # Network I/O
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

## 🔍 Logging and Monitoring

### Structured Logging
```python
import logging
import json
from datetime import datetime

class StructuredLogger:
    def __init__(self, name, log_file=None):
        self.logger = logging.getLogger(name)
        self.logger.setLevel(logging.INFO)
        
        # Create formatter
        formatter = logging.Formatter(
            '%(asctime)s - %(name)s - %(levelname)s - %(message)s'
        )
        
        # Console handler
        console_handler = logging.StreamHandler()
        console_handler.setFormatter(formatter)
        self.logger.addHandler(console_handler)
        
        # File handler
        if log_file:
            file_handler = logging.FileHandler(log_file)
            file_handler.setFormatter(formatter)
            self.logger.addHandler(file_handler)
    
    def log_task_event(self, event_type, task_id, details=None):
        """Log task-related events"""
        log_data = {
            'timestamp': datetime.utcnow().isoformat(),
            'event_type': event_type,
            'task_id': task_id,
            'node_id': self.node_id,
            'details': details or {}
        }
        
        self.logger.info(json.dumps(log_data))
    
    def log_system_event(self, event_type, details=None):
        """Log system-related events"""
        log_data = {
            'timestamp': datetime.utcnow().isoformat(),
            'event_type': event_type,
            'node_id': self.node_id,
            'details': details or {}
        }
        
        self.logger.info(json.dumps(log_data))
```

### Health Check
```python
class HealthChecker:
    def __init__(self, worker_node):
        self.worker_node = worker_node
        
    def check_health(self):
        """Perform health check"""
        health_status = {
            'status': 'healthy',
            'timestamp': time.time(),
            'checks': {}
        }
        
        # Check Node Pool connection
        health_status['checks']['node_pool'] = self._check_node_pool_connection()
        
        # Check Docker service
        health_status['checks']['docker'] = self._check_docker_service()
        
        # Check system resources
        health_status['checks']['resources'] = self._check_system_resources()
        
        # Check disk space
        health_status['checks']['disk_space'] = self._check_disk_space()
        
        # Determine overall status
        if any(check['status'] == 'unhealthy' for check in health_status['checks'].values()):
            health_status['status'] = 'unhealthy'
        
        return health_status
    
    def _check_node_pool_connection(self):
        """Check Node Pool connection"""
        try:
            self.worker_node.node_pool_client.ping()
            return {'status': 'healthy', 'message': 'Node Pool connection OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Node Pool connection failed: {str(e)}'}
    
    def _check_docker_service(self):
        """Check Docker service"""
        try:
            docker_client = docker.from_env()
            docker_client.ping()
            return {'status': 'healthy', 'message': 'Docker service OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Docker service failed: {str(e)}'}
    
    def _check_system_resources(self):
        """Check system resources"""
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
        """Check disk space"""
        try:
            disk = psutil.disk_usage('/')
            if disk.percent > 90:
                return {'status': 'unhealthy', 'message': f'Low disk space: {disk.percent}% used'}
            
            return {'status': 'healthy', 'message': 'Disk space OK'}
        except Exception as e:
            return {'status': 'unhealthy', 'message': f'Disk check failed: {str(e)}'}
```

## 🔧 Common Troubleshooting

### 1. Docker Container Execution Failure
**Problem**: Container startup or execution failure
**Solution**:
```bash
# Check Docker service status
sudo systemctl status docker

# Check if Docker images exist
docker images

# Check container logs
docker logs <container_id>

# Clean up stopped containers
docker container prune
```

### 2. Node Pool Connection Issues
**Problem**: Unable to connect to Node Pool service
**Solution**:
```python
# Implement connection retry mechanism
import time
import grpc

def connect_with_retry(address, max_retries=5, retry_delay=2):
    for attempt in range(max_retries):
        try:
            channel = grpc.insecure_channel(address)
            # Test connection
            grpc.channel_ready_future(channel).result(timeout=10)
            return channel
        except grpc.FutureTimeoutError:
            if attempt < max_retries - 1:
                print(f"Connection failed, retrying in {retry_delay} seconds...")
                time.sleep(retry_delay)
                retry_delay *= 2  # Exponential backoff
            else:
                raise Exception("Unable to connect to Node Pool")
```

### 3. Insufficient Resources Problem
**Problem**: Insufficient system resources to execute tasks
**Solution**:
```python
# Implement resource checking and waiting mechanism
def wait_for_resources(required_resources, timeout=300):
    start_time = time.time()
    
    while time.time() - start_time < timeout:
        if resource_monitor.is_resource_available(required_resources):
            return True
        
        print("Insufficient resources, waiting...")
        time.sleep(10)
    
    return False

# Usage
if wait_for_resources({'memory_gb': 2, 'cpu_cores': 1}):
    execute_task(task)
else:
    report_task_failed(task_id, "Insufficient resources")
```

## 📈 Performance Optimization

### Task Execution Optimization
```python
# Task preloading
class TaskPreloader:
    def __init__(self, docker_client):
        self.docker_client = docker_client
        self.preloaded_images = set()
    
    def preload_image(self, image_name):
        """Preload Docker image"""
        if image_name not in self.preloaded_images:
            try:
                self.docker_client.images.pull(image_name)
                self.preloaded_images.add(image_name)
                print(f"Preloaded image: {image_name}")
            except Exception as e:
                print(f"Image preload failed: {e}")

# Parallel task execution
from concurrent.futures import ThreadPoolExecutor, as_completed

class ParallelTaskExecutor:
    def __init__(self, max_concurrent_tasks=2):
        self.max_concurrent_tasks = max_concurrent_tasks
        self.executor = ThreadPoolExecutor(max_workers=max_concurrent_tasks)
        self.running_tasks = {}
    
    def submit_task(self, task):
        """Submit task for parallel execution"""
        if len(self.running_tasks) < self.max_concurrent_tasks:
            future = self.executor.submit(self.execute_task, task)
            self.running_tasks[task['id']] = future
            return True
        return False
    
    def check_completed_tasks(self):
        """Check completed tasks"""
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

### Resource Usage Optimization
```python
# Memory management
import gc

class MemoryManager:
    def __init__(self, memory_threshold=80):
        self.memory_threshold = memory_threshold
    
    def check_memory_usage(self):
        """Check memory usage"""
        memory = psutil.virtual_memory()
        return memory.percent
    
    def cleanup_memory(self):
        """Clean up memory"""
        gc.collect()
        
        # Clean up unused Docker images
        docker_client = docker.from_env()
        docker_client.images.prune()
        
        # Clean up unused containers
        docker_client.containers.prune()

# Disk space management
class DiskManager:
    def __init__(self, cleanup_threshold=85):
        self.cleanup_threshold = cleanup_threshold
    
    def cleanup_old_files(self, directory, days=7):
        """Clean up old files"""
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
                        print(f"Deleted old file: {file_path}")
                    except Exception as e:
                        print(f"File deletion failed: {e}")
```

---

**Related Documentation**:
- [Node Pool Module](node-pool.md)
- [Master Node Module](master-node.md)
- [TaskWorker Module](taskworker.md)
- [API Documentation](../api.md)
- [Deployment Guide](../deployment.md)

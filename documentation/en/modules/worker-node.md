# Worker Node Module Documentation

HiveMind Worker Node is the core execution unit of the distributed computing platform, providing enterprise-grade task execution, resource management, and monitoring capabilities.

## 📋 Overview

Worker Node is a complete distributed computing execution node with the following features:

### Core Features
- **🎯 Multi-Task Parallel Execution**: Support for concurrent execution of multiple computational tasks
- **🐳 Docker Containerized Execution**: Secure isolated task execution environment
- **🔗 Automatic VPN Connection**: Auto-connect to HiveMind distributed network
- **📊 Real-time Resource Monitoring**: CPU, memory, GPU real-time monitoring
- **🌐 Web Management Interface**: Modern node management interface
- **⚡ Trust Scoring System**: Dynamic trust scoring based on node performance
- **🔒 User Session Management**: Secure multi-user session support

### Technical Architecture

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

## 🗂️ New Architecture File Structure

```
worker/
├── src/
│   └── hivemind_worker/           # Main Python package
│       ├── worker_node.py         # Core worker node implementation
│       ├── nodepool_pb2.py        # gRPC Protocol Buffers
│       ├── nodepool_pb2_grpc.py   # gRPC service client
│       ├── __init__.py            # Package initialization
│       ├── static/                # Web interface static resources
│       │   ├── css/              # Style files
│       │   ├── js/               # JavaScript files
│       │   └── images/           # Image resources
│       └── templates/             # Flask HTML templates
│           ├── dashboard.html     # Main dashboard
│           ├── login.html         # Login page
│           └── tasks.html         # Task management page
├── main.py                        # Main entry point
├── pyproject.toml                 # Python project configuration
├── requirements.txt               # Dependencies
├── Dockerfile                     # Docker build file
├── run_task.sh                    # Task execution script
├── nodepool.proto                 # gRPC service definitions
└── README.md                      # Documentation
```

## 🔧 Core Components Detailed

### 1. Main WorkerNode Class

```python
class WorkerNode:
    def __init__(self):
        # Basic node information
        self.node_id = NODE_ID
        self.port = NODE_PORT
        self.master_address = MASTER_ADDRESS
        
        # Multi-task execution system
        self.running_tasks = {}     # {task_id: task_info}
        self.task_locks = {}        # {task_id: threading.Lock()}
        self.task_stop_events = {}  # {task_id: Event()}
        
        # Resource management system
        self.available_resources = {
            "cpu": 0,               # CPU score
            "memory_gb": 0,         # Available memory GB
            "gpu": 0,               # GPU score  
            "gpu_memory_gb": 0      # GPU memory GB
        }
        
        # Trust and security system
        self.trust_score = 0        # 0-100 trust score
        self.trust_group = "low"    # high/medium/low
        self.user_sessions = {}     # Multi-user sessions
```

### 2. Multi-Task Execution Engine

**Parallel Task Support**:
```python
def execute_task(self, task_id, task_data):
    """Execute task (supports parallelism)"""
    # 1. Create task-specific lock and stop event
    self.task_locks[task_id] = threading.Lock()
    self.task_stop_events[task_id] = threading.Event()
    
    # 2. Allocate resources
    required_resources = self._calculate_task_resources(task_data)
    if not self._allocate_resources(task_id, required_resources):
        return {"error": "Insufficient resources"}
    
    # 3. Execute task in separate thread
    task_thread = threading.Thread(
        target=self._run_task_in_thread,
        args=(task_id, task_data),
        name=f"Task-{task_id}"
    )
    task_thread.start()
    
    return {"status": "started", "task_id": task_id}

def _run_task_in_thread(self, task_id, task_data):
    """Execute task in separate thread"""
    try:
        # Update task status to running
        with self.task_locks[task_id]:
            self.running_tasks[task_id] = {
                "status": "RUNNING",
                "start_time": time(),
                "resources": task_data.get("required_resources", {})
            }
        
        # Execute Docker container task
        result = self._execute_docker_task(task_id, task_data)
        
        # Update completion status
        with self.task_locks[task_id]:
            self.running_tasks[task_id]["status"] = "COMPLETED"
            self.running_tasks[task_id]["result"] = result
            
    except Exception as e:
        with self.task_locks[task_id]:
            self.running_tasks[task_id]["status"] = "FAILED"
            self.running_tasks[task_id]["error"] = str(e)
    finally:
        # Release resources
        self._release_task_resources(task_id)
```

### 3. Dynamic Resource Management

**Intelligent Resource Allocation**:
```python
def _allocate_resources(self, task_id, required_resources):
    """Allocate resources for task"""
    with self.resources_lock:
        # Check if sufficient resources available
        if (self.available_resources["cpu"] >= required_resources.get("cpu", 0) and
            self.available_resources["memory_gb"] >= required_resources.get("memory_gb", 0) and
            self.available_resources["gpu"] >= required_resources.get("gpu", 0)):
            
            # Allocate resources
            for resource, amount in required_resources.items():
                self.available_resources[resource] -= amount
            
            # Record task resource usage
            self.running_tasks[task_id] = {
                "status": "ALLOCATED",
                "resources": required_resources,
                "start_time": time()
            }
            return True
        
        return False

def _release_task_resources(self, task_id):
    """Release task resources"""
    with self.resources_lock:
        if task_id in self.running_tasks:
            task_resources = self.running_tasks[task_id].get("resources", {})
            
            # Return resources
            for resource, amount in task_resources.items():
                if resource in self.available_resources:
                    self.available_resources[resource] += amount
                    # Ensure not exceeding total resource limits
                    self.available_resources[resource] = min(
                        self.available_resources[resource],
                        self.total_resources[resource]
                    )
```

### 4. VPN Auto-Connection System

**Automatic Network Joining**:
```python
def _auto_join_vpn(self):
    """Auto-connect to HiveMind VPN network"""
    try:
        # Check if already connected to VPN
        if self._check_vpn_connection():
            self._log("Already connected to HiveMind network")
            return
        
        # Guide user for manual connection (non-interactive mode)
        self._log("VPN connection required for HiveMind network")
        self._log("Please ensure WireGuard VPN is configured and running")
        
        # Wait for user confirmation
        print("If you have already connected, please press y")
        response = input()
        if response.lower() == 'y':
            self._log("VPN connection confirmed")
        
    except Exception as e:
        self._log(f"VPN setup failed: {e}", WARNING)

def _get_local_ip(self):
    """Get local IP (prioritize WireGuard interface)"""
    try:
        interfaces_list = interfaces()
        
        # Prioritize WireGuard interfaces
        wg_interfaces = [iface for iface in interfaces_list 
                        if 'wg' in iface.lower() or 'wireguard' in iface.lower()]
        
        if wg_interfaces:
            for wg_iface in wg_interfaces:
                addrs = ifaddresses(wg_iface)
                if AF_INET in addrs:
                    return addrs[AF_INET][0]['addr']
        
        # Check 10.0.0.x VPN subnet
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

## 🌐 Web Management Interface

Worker Node provides a modern Web management interface supporting:

### Main Functional Pages

1. **Dashboard (`/dashboard`)**
   - Real-time node status monitoring
   - Resource utilization charts
   - Task execution statistics
   - System performance metrics

2. **Task Management (`/tasks`)**
   - Current running task list
   - Task history records
   - Task detailed information
   - Task stop controls

3. **System Monitoring (`/monitor`)**
   - CPU utilization
   - Memory usage status
   - Docker status
   - Network connection status

### RESTful API Endpoints

```python
# System status API
@app.route('/api/status')
def api_status():
    """Get complete node status information"""
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

# Task management API
@app.route('/api/tasks')
def api_tasks():
    """Get all running tasks"""
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

# Task control API
@app.route('/api/stop_task/<task_id>', methods=['POST'])
def api_stop_task(task_id):
    """Stop specified task"""
    if task_id in self.task_stop_events:
        self.task_stop_events[task_id].set()
        return jsonify({'success': True, 'message': f'Task {task_id} stopped'})
    return jsonify({'success': False, 'error': 'Task not found'}), 404
```

## 🐳 Docker Container Management

### Containerized Task Execution

```python
def _execute_docker_task(self, task_id, task_data):
    """Execute task in Docker container"""
    try:
        # 1. Prepare work directory
        work_dir = self._prepare_task_workspace(task_id, task_data)
        
        # 2. Create Docker container
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
        
        # 3. Monitor container execution
        while container.status != 'exited':
            # Check stop signal
            if self.task_stop_events[task_id].is_set():
                container.stop(timeout=10)
                break
                
            sleep(1)
            container.reload()
        
        # 4. Collect execution results
        logs = container.logs(stdout=True, stderr=True).decode('utf-8')
        exit_code = container.attrs['State']['ExitCode']
        
        # 5. Collect output files
        result_files = self._collect_task_results(container, work_dir)
        
        # 6. Cleanup container
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

## 📊 Trust Scoring System

Worker Node implements a dynamic trust scoring mechanism:

### Trust Score Calculation

```python
def update_trust_score(self, task_result):
    """Update node trust score"""
    if task_result.get('status') == 'completed':
        # Successfully completed tasks increase trust score
        self.trust_score = min(100, self.trust_score + 2)
    elif task_result.get('status') == 'failed':
        # Failed tasks decrease trust score
        self.trust_score = max(0, self.trust_score - 5)
    
    # Update trust group
    if self.trust_score >= 80:
        self.trust_group = "high"
    elif self.trust_score >= 50:
        self.trust_group = "medium"
    else:
        self.trust_group = "low"
    
    self._log(f"Trust score updated: {self.trust_score} (Group: {self.trust_group})")
```

## 🚀 Deployment and Configuration

### Environment Variable Configuration

```bash
# Basic configuration
NODE_PORT=50053                    # gRPC service port
FLASK_PORT=5000                    # Web interface port
MASTER_ADDRESS=10.0.0.1:50051     # Master Node address
NODE_ID=worker-hostname-50053      # Node unique identifier

# Docker configuration
DOCKER_ENABLED=true                # Enable Docker support
DOCKER_NETWORK=hivemind           # Docker network name

# Resource configuration
MAX_CPU_CORES=4                   # Maximum CPU cores
MAX_MEMORY_GB=8                   # Maximum memory GB
MAX_CONCURRENT_TASKS=3            # Maximum concurrent tasks

# Security configuration
SESSION_TIMEOUT=24                # Session timeout (hours)
TRUST_THRESHOLD=50               # Trust score threshold
```

### Using Python Package Installation

```bash
# 1. Install from source
cd worker/
pip install -e .

# 2. Use pre-built package
pip install hivemind_worker==0.0.7

# 3. Run Worker Node
python -m hivemind_worker.worker_node
```

### Docker Deployment

```bash
# 1. Build Docker image
docker build -t hivemind-worker:latest .

# 2. Run container
docker run -d \
  --name hivemind-worker \
  --network host \
  -e MASTER_ADDRESS=10.0.0.1:50051 \
  -e NODE_PORT=50053 \
  -e FLASK_PORT=5000 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  hivemind-worker:latest
```

## 🔍 Monitoring and Logging

### Logging System

Worker Node provides comprehensive logging:

```python
def _log(self, message, level=INFO):
    """Thread-safe logging"""
    with self.log_lock:
        timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        log_entry = f"[{timestamp}] {getLevelName(level)}: {message}"
        
        # Add to memory logs
        self.logs.append({
            'timestamp': timestamp,
            'level': getLevelName(level),
            'message': message
        })
        
        # Maintain log size limit
        if len(self.logs) > 1000:
            self.logs = self.logs[-500:]  # Keep latest 500 entries
        
        print(log_entry)
```

### Health Checks

```python
@app.route('/health')
def health_check():
    """Health check endpoint"""
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

## 🔧 Common Troubleshooting

### 1. Docker Connection Issues

```bash
# Check Docker service status
systemctl status docker

# Restart Docker service
sudo systemctl restart docker

# Check Docker permissions
sudo usermod -aG docker $USER
```

### 2. VPN Connection Issues

```bash
# Check WireGuard status
sudo wg show

# Restart WireGuard
sudo systemctl restart wg-quick@wg0

# Check routing table
ip route | grep 10.0.0
```

### 3. Resource Shortage Issues

```python
# Check system resources
@app.route('/api/resources')
def check_resources():
    return jsonify({
        'cpu_available': psutil.cpu_percent(interval=1),
        'memory_available': psutil.virtual_memory().available / (1024**3),
        'disk_available': psutil.disk_usage('/').free / (1024**3),
        'running_tasks': len(self.running_tasks)
    })
```

### 4. Task Execution Failures

Check task logs:
```bash
# Check container logs
docker logs task-{task_id}

# Check work directory
ls -la /tmp/hivemind/tasks/{task_id}/
```

## 📈 Performance Optimization

### 1. Resource Scheduling Optimization

```python
def _optimize_resource_allocation(self):
    """Optimize resource allocation strategy"""
    # Adjust resource allocation based on historical data
    cpu_utilization = self._get_average_cpu_usage()
    memory_utilization = self._get_average_memory_usage()
    
    # Dynamically adjust available resources
    if cpu_utilization < 0.7:
        self.available_resources['cpu'] = min(
            self.total_resources['cpu'],
            self.available_resources['cpu'] * 1.1
        )
```

### 2. Network Optimization

```python
def _optimize_network_settings(self):
    """Optimize network settings"""
    # Adjust gRPC connection parameters
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

This updated Worker Node documentation reflects the latest architectural improvements, including multi-task support, trust scoring system, VPN auto-connection, and modernized Web interface.

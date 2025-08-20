# Master Node Module Documentation

## 📋 Overview

The Master Node is the management center of the HiveMind distributed computing platform, providing user interface, task management, and system monitoring capabilities. It serves as the control plane for the entire system, coordinating the work of various components.

## 🏗️ System Architecture

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

## 🔧 Core Components

### 1. Master Node Server (`master_node.py`)
- **Function**: Main Flask web application server
- **Port**: 5000
- **Protocol**: HTTP/HTTPS Web Interface

**Main Features**:
```python
# Web routes
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
- **Function**: gRPC communication interface with Node Pool
- **Responsibilities**:
  - Task submission and management
  - Node status queries
  - User authentication relay

### 3. VPN Management (`vpn.py`)
- **Function**: WireGuard VPN configuration management
- **Responsibilities**:
  - VPN configuration file generation
  - Client connection management
  - Network security tunnels

### 4. Task Splitter (`task_splitter.py`)
- **Function**: Large task decomposition and allocation
- **Responsibilities**:
  - Task sharding algorithms
  - Load balancing strategies
  - Result aggregation

## 🌐 Web Interface Features

### User Authentication System
- **Login Page**: User identity authentication
- **Session Management**: Session and Cookie handling
- **Access Control**: Role-based access control

### Task Management Interface
- **Task Upload**: File upload and task configuration
- **Task Monitoring**: Real-time status and progress display
- **Result Download**: Retrieve results from completed tasks

### Node Monitoring Panel
- **Node Status**: Active node list and status
- **Performance Metrics**: CPU, memory, network usage
- **Historical Data**: Node performance history charts

### System Dashboard
- **System Overview**: Overall system health status
- **Statistics**: Task execution statistics and trends
- **Alert Notifications**: System exceptions and warning messages

## 📡 Communication with Node Pool

### gRPC Client Implementation
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

## 🗂️ File Structure

```
master/
├── master_node.py              # Main Flask application
├── hivemind_integration.py     # Node Pool integration
├── vpn.py                      # VPN management
├── task_splitter.py           # Task decomposer
├── nodepool_pb2.py            # Protocol Buffer generated files
├── nodepool_pb2_grpc.py       # gRPC generated files
├── requirements.txt           # Python dependencies
├── file.ico                   # Application icon
├── templates_master/          # Jinja2 templates
│   ├── login.html            # Login page
│   ├── master_dashboard.html # Main dashboard
│   └── master_upload.html    # Task upload page
└── HiveMind-master-Release/   # Release package
    ├── install.sh            # Installation script
    ├── start_hivemind.cmd    # Windows startup script
    └── ...
```

## 🚀 Deployment and Configuration

### Local Development Environment
```bash
cd master
pip install -r requirements.txt
python master_node.py
```

### Production Environment Deployment
```bash
# Using Gunicorn
gunicorn -w 4 -b 0.0.0.0:5000 master_node:app

# Using systemd service
sudo systemctl enable hivemind-master
sudo systemctl start hivemind-master
```

### Configuration File Example
```python
# config.py
import os

class Config:
    SECRET_KEY = os.environ.get('SECRET_KEY') or 'dev-secret-key'
    NODE_POOL_ADDRESS = os.environ.get('NODE_POOL_ADDRESS') or 'localhost:50051'
    UPLOAD_FOLDER = os.environ.get('UPLOAD_FOLDER') or './uploads'
    MAX_CONTENT_LENGTH = 16 * 1024 * 1024  # 16MB
    
    # VPN Configuration
    VPN_SERVER_IP = os.environ.get('VPN_SERVER_IP') or '10.0.0.1'
    VPN_PORT = int(os.environ.get('VPN_PORT') or 51820)
    
    # Database Configuration
    DATABASE_URL = os.environ.get('DATABASE_URL') or 'sqlite:///master.db'
```

## 🔐 VPN Management

### WireGuard Configuration Generation
```python
def generate_vpn_config(user_id, client_ip):
    """Generate WireGuard VPN configuration for user"""
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

### VPN Client Management
- **Configuration Distribution**: Automatic client configuration file generation
- **Connection Monitoring**: Client connection status tracking
- **Access Control**: User permission-based network access

## 📊 Task Management System

### Task Lifecycle
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
        """Submit new task to system"""
        task_id = self.generate_task_id()
        
        # Task decomposition
        subtasks = self.split_task(task_data)
        
        # Submit to Node Pool
        for subtask in subtasks:
            self.submit_subtask(task_id, subtask)
        
        return task_id
    
    def monitor_task(self, task_id):
        """Monitor task execution status"""
        subtasks = self.get_subtasks(task_id)
        
        total = len(subtasks)
        completed = sum(1 for st in subtasks if st.status == TaskStatus.COMPLETED)
        
        return {
            'progress': (completed / total) * 100,
            'status': self.calculate_overall_status(subtasks)
        }
```

### Task Decomposition Strategy
```python
def split_task(self, task_data):
    """Decompose tasks based on task type"""
    if task_data['type'] == 'data_processing':
        return self.split_data_processing_task(task_data)
    elif task_data['type'] == 'machine_learning':
        return self.split_ml_task(task_data)
    elif task_data['type'] == 'simulation':
        return self.split_simulation_task(task_data)
    else:
        return [task_data]  # No decomposition
```

## 🎨 Frontend Interface

### HTML Template Structure
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
            <a href="/dashboard">Dashboard</a>
            <a href="/tasks">Task Management</a>
            <a href="/nodes">Node Monitoring</a>
            <a href="/vpn">VPN Management</a>
        </div>
    </nav>
    
    <main class="dashboard">
        <!-- Dashboard content -->
    </main>
    
    <script src="/static/js/dashboard.js"></script>
</body>
</html>
```

### JavaScript Features
```javascript
// static/js/dashboard.js
class Dashboard {
    constructor() {
        this.updateInterval = 5000; // 5-second updates
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

// Initialize dashboard
document.addEventListener('DOMContentLoaded', () => {
    new Dashboard();
});
```

## 🔍 Monitoring and Logging

### System Metrics Collection
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

### Logging Configuration
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

## 🛠️ API Endpoints

### RESTful API
```python
# API routes
@app.route('/api/tasks', methods=['GET'])
def api_get_tasks():
    """Get task list"""
    return jsonify({'tasks': task_manager.get_all_tasks()})

@app.route('/api/tasks', methods=['POST'])
def api_submit_task():
    """Submit new task"""
    task_data = request.json
    task_id = task_manager.submit_task(task_data)
    return jsonify({'task_id': task_id})

@app.route('/api/nodes', methods=['GET'])
def api_get_nodes():
    """Get node list"""
    nodes = hivemind_client.get_node_list()
    return jsonify({'nodes': nodes})

@app.route('/api/system/status', methods=['GET'])
def api_system_status():
    """Get system status"""
    return jsonify(system_monitor.get_system_metrics())
```

## 🔧 Common Troubleshooting

### 1. Flask Application Startup Failure
**Problem**: `Address already in use`
**Solution**:
```bash
# Find process using port 5000
lsof -i :5000

# Kill process
kill -9 <PID>
```

### 2. Node Pool Connection Failure
**Problem**: gRPC connection timeout
**Solution**:
```python
# Implement connection retry mechanism
import time
from grpc import RpcError

def connect_to_node_pool(max_retries=3):
    for attempt in range(max_retries):
        try:
            channel = grpc.insecure_channel('localhost:50051')
            stub = nodepool_pb2_grpc.NodeManagerStub(channel)
            # Test connection
            stub.GetNodeList(nodepool_pb2.GetNodeListRequest())
            return stub
        except RpcError:
            if attempt < max_retries - 1:
                time.sleep(2 ** attempt)  # Exponential backoff
            else:
                raise
```

### 3. VPN Configuration Issues
**Problem**: WireGuard connection failure
**Solution**:
```bash
# Check WireGuard service status
sudo systemctl status wg-quick@wg0

# Check firewall settings
sudo ufw allow 51820/udp

# Check routing table
ip route show
```

## 📈 Performance Optimization

### Flask Application Optimization
```python
# Use Redis for session storage
from flask_session import Session
import redis

app.config['SESSION_TYPE'] = 'redis'
app.config['SESSION_REDIS'] = redis.from_url('redis://localhost:6379')
Session(app)

# Enable gzip compression
from flask_compress import Compress
Compress(app)

# Static file caching
@app.after_request
def after_request(response):
    if request.endpoint == 'static':
        response.cache_control.max_age = 86400  # 1 day
    return response
```

### Frontend Optimization
- **Resource Compression**: JavaScript and CSS file compression
- **Caching Strategy**: Browser caching and CDN configuration
- **Asynchronous Loading**: Ajax requests and asynchronous page updates

## 🔄 Maintenance and Updates

### Automated Deployment Script
```bash
#!/bin/bash
# deploy_master.sh

# Stop existing service
sudo systemctl stop hivemind-master

# Update code
git pull origin main

# Install dependencies
pip install -r requirements.txt

# Database migration (if needed)
python manage.py db upgrade

# Restart service
sudo systemctl start hivemind-master

# Check status
sudo systemctl status hivemind-master
```

### Health Check
```python
@app.route('/health')
def health_check():
    """System health check endpoint"""
    try:
        # Check Node Pool connection
        hivemind_client.ping()
        
        # Check database connection
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

**Related Documentation**:
- [Node Pool Module](node-pool.md)
- [Worker Node Module](worker-node.md)
- [API Documentation](../api.md)
- [Deployment Guide](../deployment.md)

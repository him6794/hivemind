# HiveMind Developer Guide

## Overview

Welcome to the HiveMind distributed computing platform developer guide! This document will help you understand the project architecture, development environment setup, coding standards, and contribution workflow.

## Project Architecture

### Overall Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    HiveMind Distributed Computing Platform   │
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
│  │ Model Split │    │ P2P Transfer│    │ Official Site│      │
│  │ Smart Sched │    │ Seed Mgmt   │    │ User Reg    │      │
│  │ (In Dev)    │    │ (Complete)  │    │ (Online)    │      │
│  └─────────────┘    └─────────────┘    └─────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### Project Structure

```
hivemind/
├── 📁 node_pool/                   # Node pool service (Core component)
│   ├── node_pool_server.py         # Main server entry (594 lines)
│   ├── user_service.py             # User authentication service (292 lines)
│   ├── node_manager_service.py     # Node management service (1714 lines)
│   ├── master_node_service.py      # Master node service (1714 lines)
│   ├── database_manager.py         # Database management (125 lines)
│   ├── config.py                   # Configuration management (31 lines)
│   ├── nodepool_pb2.py            # gRPC protocol definition
│   └── nodepool_pb2_grpc.py       # gRPC service stubs
│
├── 📁 master/                      # Master control node
│   ├── master_node.py             # Master node main program (1798 lines)
│   ├── vpn.py                     # VPN management functionality (315 lines)
│   ├── nodepool_pb2.py           # Protocol definition
│   └── templates_master/          # Web templates
│
├── 📁 worker/                      # Worker nodes
│   ├── worker_node.py             # Worker node main program (1798 lines)
│   ├── nodepool_pb2.py           # Protocol definition
│   ├── nodepool.proto            # Protocol Buffers definition
│   └── static/, templates/        # Web resources
│
├── 📁 taskworker/                  # Task worker library
│   ├── worker.py                  # Task executor (292 lines)
│   ├── storage.py                 # Storage management (166 lines)
│   ├── dns_proxy.py              # DNS proxy (103 lines)
│   ├── rpc_service.py            # RPC service (61 lines)
│   └── protos/                   # gRPC protocol definitions
│
├── 📁 ai/                          # AI module (In development)
│   ├── main.py                   # AI main program (61 lines)
│   ├── breakdown.py              # Task splitting (47 lines)
│   └── Identification.py         # Identity recognition (30 lines)
│
├── 📁 bt/                          # BT module
│   ├── tracker.py                # BitTorrent tracker (134 lines)
│   ├── seeder.py                 # Seed server (88 lines)
│   └── create_torrent.py         # Torrent creation tool (47 lines)
│
├── 📁 web/                         # Web service
│   ├── app.py                    # Flask web application (205 lines)
│   ├── vpn_service.py           # VPN service (135 lines)
│   ├── wireguard_server.py      # WireGuard server (92 lines)
│   └── static/, templates/       # Web resources
│
└── 📁 documentation/               # Unified documentation center
    ├── zh-tw/                    # Traditional Chinese documentation
    └── en/                       # English documentation
```

### Core Component Description

#### 1. Node Pool
- **Function**: System core, responsible for node management, task scheduling, user authentication
- **Main File**: `node_pool_server.py` (594 lines)
- **gRPC Service**: Provides complete node management API
- **Data Storage**: SQLite (users) + Redis (state)

#### 2. Master Node
- **Function**: Provides management interface, VPN management, system monitoring
- **Main File**: `master_node.py` (1798 lines)
- **Features**: Web interface, VPN configuration, task monitoring

#### 3. Worker Node
- **Function**: Executes actual computation tasks, reports status
- **Main File**: `worker_node.py` (1798 lines)
- **Features**: Task execution, resource monitoring, result reporting

#### 4. TaskWorker
- **Function**: Independent task execution library, can be integrated by other projects
- **Main File**: `worker.py` (292 lines)
- **Characteristics**: Lightweight, scalable, independent deployment

## Development Environment Setup

### 1. System Requirements

- **Python**: 3.8+
- **Redis**: 6.0+
- **Git**: 2.20+
- **OS**: Linux/macOS/Windows

### 2. Clone Project

```bash
git clone https://github.com/him6794/hivemind.git
cd hivemind
```

### 3. Setup Virtual Environment

```bash
# Create virtual environment
python -m venv hivemind-env

# Activate virtual environment
# Linux/macOS:
source hivemind-env/bin/activate
# Windows:
hivemind-env\Scripts\activate
```

### 4. Install Dependencies

```bash
# Install global dependencies
pip install -r requirements.txt

# Install development dependencies
pip install pytest pytest-cov black flake8 mypy

# Install module-specific dependencies
pip install -r master/requirements.txt
pip install -r worker/requirements.txt
pip install -r taskworker/requirements.txt
```

### 5. Setup Redis

```bash
# Ubuntu/Debian
sudo apt update && sudo apt install redis-server

# macOS
brew install redis

# Start Redis
redis-server
```

### 6. Configure Environment Variables

Create `.env` file:

```bash
# Node Pool configuration
NODEPOOL_HOST=localhost
NODEPOOL_PORT=50051
REDIS_HOST=localhost
REDIS_PORT=6379

# Development mode
DEBUG=true
LOG_LEVEL=DEBUG

# Test database
TEST_DB_PATH=test_users.db
```

## Code Structure Explanation

### gRPC Protocol Definition

```protobuf
// nodepool.proto
service NodePoolService {
  // Node management
  rpc RegisterNode(RegisterNodeRequest) returns (RegisterNodeResponse);
  rpc UnregisterNode(UnregisterNodeRequest) returns (UnregisterNodeResponse);
  
  // User management
  rpc RegisterUser(RegisterUserRequest) returns (RegisterUserResponse);
  rpc LoginUser(LoginUserRequest) returns (LoginUserResponse);
  
  // Task management
  rpc SubmitTask(SubmitTaskRequest) returns (SubmitTaskResponse);
  rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
}
```

### Main Class Structure

#### Node Pool Server
```python
class NodePoolServer:
    def __init__(self):
        self.redis_client = redis.Redis()
        self.user_service = UserService()
        self.node_manager = NodeManagerService()
        
    def serve(self):
        # Start gRPC server
        
class UserService:
    def RegisterUser(self, request, context):
        # User registration logic
        
class NodeManagerService:
    def RegisterNode(self, request, context):
        # Node registration logic
```

#### Worker Node
```python
class WorkerNode:
    def __init__(self, node_id):
        self.node_id = node_id
        self.executor = TaskExecutor()
        self.monitor = ResourceMonitor()
        
    def start(self):
        # Start worker node
        
class TaskExecutor:
    def execute_task(self, task_data):
        # Task execution logic
```

## Development Workflow

### 1. Branching Strategy

```bash
main           # Stable version
├── develop    # Main development branch
├── feature/*  # Feature branches
├── hotfix/*   # Hotfix branches
└── release/*  # Release branches
```

### 2. Feature Development Process

```bash
# 1. Create feature branch
git checkout develop
git pull origin develop
git checkout -b feature/new-feature

# 2. Development and testing
# ... Write code ...
python -m pytest tests/

# 3. Code checking
black .
flake8 .
mypy .

# 4. Commit code
git add .
git commit -m "feat: add new feature"

# 5. Push and create PR
git push origin feature/new-feature
# Create Pull Request on GitHub
```

### 3. Coding Standards

#### Python Code Style

```python
# Use Black formatting
# Use type hints
def process_task(task_data: Dict[str, Any]) -> TaskResult:
    """Process task data
    
    Args:
        task_data: Task input data
        
    Returns:
        Processing result
        
    Raises:
        TaskExecutionError: Raised when task execution fails
    """
    pass

# Use dataclass to define data structures
@dataclass
class TaskConfig:
    timeout: int = 300
    max_retries: int = 3
    priority: str = "normal"
```

#### Naming Conventions

- **File names**: `snake_case.py`
- **Class names**: `PascalCase`
- **Functions/Variables**: `snake_case`
- **Constants**: `UPPER_SNAKE_CASE`
- **Private members**: `_leading_underscore`

#### Docstrings

```python
def register_node(node_info: NodeInfo) -> bool:
    """Register a new worker node
    
    Args:
        node_info: Node information including ID, address, capabilities
        
    Returns:
        True if registration successful, False if failed
        
    Example:
        >>> node_info = NodeInfo(id="worker-1", host="192.168.1.100")
        >>> success = register_node(node_info)
        >>> print(success)
        True
    """
    pass
```

## Testing Strategy

### 1. Test Structure

```
tests/
├── unit/                    # Unit tests
│   ├── test_user_service.py
│   ├── test_node_manager.py
│   └── test_task_executor.py
├── integration/             # Integration tests
│   ├── test_grpc_services.py
│   └── test_worker_integration.py
├── e2e/                     # End-to-end tests
│   └── test_full_workflow.py
└── fixtures/                # Test fixtures
    ├── sample_tasks.json
    └── test_data.py
```

### 2. Test Examples

```python
# tests/unit/test_user_service.py
import pytest
from node_pool.user_service import UserService

class TestUserService:
    def setup_method(self):
        self.user_service = UserService(test_mode=True)
        
    def test_register_user_success(self):
        """Test successful user registration"""
        request = RegisterUserRequest(
            username="testuser",
            password="password123",
            email="test@example.com"
        )
        response = self.user_service.RegisterUser(request, None)
        assert response.success is True
        assert response.user_id is not None
        
    def test_register_duplicate_user(self):
        """Test duplicate user registration failure"""
        # First registration
        request = RegisterUserRequest(...)
        self.user_service.RegisterUser(request, None)
        
        # Second registration should fail
        response = self.user_service.RegisterUser(request, None)
        assert response.success is False
        assert "already exists" in response.message
```

### 3. Running Tests

```bash
# Run all tests
pytest

# Run specific tests
pytest tests/unit/test_user_service.py

# Generate coverage report
pytest --cov=node_pool --cov-report=html

# Run end-to-end tests
pytest tests/e2e/ -v
```

## Deployment and CI/CD

### 1. GitHub Actions Configuration

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

### 2. Docker Deployment

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

## Extension Development

### 1. Adding New gRPC Services

```bash
# 1. Modify proto file
vim nodepool.proto

# 2. Generate Python code
python -m grpc_tools.protoc \
    --python_out=. \
    --grpc_python_out=. \
    nodepool.proto

# 3. Implement service logic
class NewService:
    def NewMethod(self, request, context):
        # Implementation logic
        pass
```

### 2. Adding New Task Types

```python
# taskworker/tasks/new_task.py
from taskworker.worker import BaseTask

class NewTaskType(BaseTask):
    def execute(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """Execute new task type"""
        # Implement task logic
        return {"result": "success"}
        
# Register task type
from taskworker.registry import register_task
register_task("new_task", NewTaskType)
```

### 3. Adding New Web API

```python
# web/api/new_endpoint.py
from flask import Blueprint, request, jsonify

new_api = Blueprint('new_api', __name__)

@new_api.route('/api/new-endpoint', methods=['POST'])
def handle_new_request():
    data = request.get_json()
    # Processing logic
    return jsonify({"status": "success"})
```

## Performance Optimization

### 1. gRPC Optimization

```python
# Connection pool configuration
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

### 2. Redis Optimization

```python
# Use connection pool
import redis.connection
pool = redis.ConnectionPool(
    host='localhost',
    port=6379,
    db=0,
    max_connections=20
)
redis_client = redis.Redis(connection_pool=pool)

# Batch operations
pipe = redis_client.pipeline()
for key, value in data.items():
    pipe.hset(key, mapping=value)
pipe.execute()
```

### 3. Asynchronous Processing

```python
import asyncio
import aioredis

async def async_task_handler():
    redis = aioredis.from_url("redis://localhost")
    
    async def process_task(task_data):
        # Asynchronous task processing
        result = await some_async_operation(task_data)
        await redis.hset("results", task_id, result)
        
    tasks = [process_task(data) for data in task_list]
    await asyncio.gather(*tasks)
```

## Monitoring and Logging

### 1. Structured Logging

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

### 2. Metrics Collection

```python
from prometheus_client import Counter, Histogram, Gauge

# Define metrics
task_counter = Counter('tasks_total', 'Total number of tasks')
task_duration = Histogram('task_duration_seconds', 'Task execution time')
active_workers = Gauge('active_workers', 'Number of active workers')

# Use metrics
@task_duration.time()
def execute_task():
    task_counter.inc()
    # Execute task
    
def update_worker_count(count):
    active_workers.set(count)
```

## Security Considerations

### 1. Authentication and Authorization

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

### 2. Input Validation

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

## Contribution Guidelines

### 1. Submitting Issues

Use the following template for bug reports:

```markdown
## Bug Description
Concise description of the issue encountered

## Steps to Reproduce
1. Execute `command`
2. Observe result

## Expected Behavior
Describe the expected correct behavior

## Actual Behavior
Describe what actually happened

## Environment Information
- OS: Ubuntu 20.04
- Python: 3.9.5
- HiveMind Version: v1.0.0
```

### 2. Submitting Pull Requests

```markdown
## Change Description
Describe the purpose and changes of this PR

## Change Type
- [ ] Bug fix
- [ ] New feature
- [ ] Documentation update
- [ ] Performance optimization

## Test Checklist
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Code coverage > 80%
- [ ] Documentation updated

## Related Issues
Closes #123
```

### 3. Code Review Checklist

**Functionality**:
- [ ] Code implements expected functionality
- [ ] Boundary conditions handled correctly
- [ ] Error handling complete

**Code Quality**:
- [ ] Code style follows standards
- [ ] Variable naming is clear
- [ ] Functions have single responsibility

**Performance**:
- [ ] No obvious performance issues
- [ ] Resource usage reasonable
- [ ] No memory leaks

**Security**:
- [ ] Input validation sufficient
- [ ] No SQL injection risks
- [ ] Sensitive information properly handled

## FAQ

### Q: How to debug gRPC services?

```python
# Enable gRPC debug logging
import os
os.environ['GRPC_VERBOSITY'] = 'DEBUG'
os.environ['GRPC_TRACE'] = 'all'

# Test with grpcurl
grpcurl -plaintext localhost:50051 list
grpcurl -plaintext localhost:50051 describe NodePoolService
```

### Q: How to simulate distributed environment testing?

```bash
# Use Docker Compose to create multi-node environment
docker-compose up --scale worker=5

# Or use Kind to create Kubernetes test environment
kind create cluster
kubectl apply -f k8s/
```

### Q: How to perform performance testing?

```python
# Use locust for load testing
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

**Updated**: January 2024  
**Version**: v1.0  
**Status**: Complete developer guide

**For Help**: Please refer to the project [Wiki](https://github.com/him6794/hivemind/wiki) or contact the development team on [Discord](https://discord.gg/hivemind).

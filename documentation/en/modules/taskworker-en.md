# TaskWorker Module (English)

## Overview

TaskWorker is a lightweight, independent task execution library that can be integrated into the HiveMind platform or used as a standalone distributed computing solution. It provides a clean interface for executing various types of computational tasks across distributed nodes.

## Architecture

```
┌─────────────────────────────────────────┐
│           TaskWorker Library             │
├─────────────────────────────────────────┤
│ ┌─────────────┐ ┌─────────────────────┐ │
│ │Task Worker  │ │   RPC Service       │ │
│ │             │ │                     │ │
│ │• Execution  │ │• gRPC Server        │ │
│ │• Management │ │• Task Distribution  │ │
│ │• Monitoring │ │• Status Reporting   │ │
│ └─────────────┘ └─────────────────────┘ │
│ ┌─────────────┐ ┌─────────────────────┐ │
│ │   Storage   │ │    DNS Proxy        │ │
│ │             │ │                     │ │
│ │• File Mgmt  │ │• Name Resolution    │ │
│ │• Caching    │ │• Service Discovery  │ │
│ │• Cleanup    │ │• Load Balancing     │ │
│ └─────────────┘ └─────────────────────┘ │
└─────────────────────────────────────────┘
```

## Core Components

### 1. Task Worker (`worker.py`)

**Responsibilities**:
- Task execution and management
- Resource monitoring
- Result handling
- Error management

**Key Features**:
- Multi-threaded task execution
- Resource usage tracking
- Task timeout handling
- Result caching

### 2. RPC Service (`rpc_service.py`)

**Responsibilities**:
- gRPC server implementation
- Task distribution handling
- Communication with HiveMind nodes

**Key Features**:
- Asynchronous task handling
- Health checks
- Metrics reporting

### 3. Storage Manager (`storage.py`)

**Responsibilities**:
- File and data management
- Temporary storage handling
- Result caching
- Cleanup operations

### 4. DNS Proxy (`dns_proxy.py`)

**Responsibilities**:
- Service discovery
- Name resolution
- Load balancing
- Network optimization

## Installation and Setup

### Standalone Installation

```bash
# Clone the TaskWorker module
git clone https://github.com/him6794/hivemind.git
cd hivemind/taskworker

# Install dependencies
pip install -r requirements.txt

# Start TaskWorker service
python rpc_service.py
```

### Integration with HiveMind

```python
# Import TaskWorker in your HiveMind worker node
from taskworker.worker import TaskWorker
from taskworker.storage import StorageManager

# Initialize TaskWorker
worker = TaskWorker(max_concurrent_tasks=8)
storage = StorageManager("/data/taskworker")
```

## Configuration

```python
class TaskWorkerConfig:
    # Server settings
    GRPC_HOST = "0.0.0.0"
    GRPC_PORT = 50053
    MAX_WORKERS = 10
    
    # Task execution settings
    MAX_CONCURRENT_TASKS = 4
    DEFAULT_TIMEOUT = 300  # 5 minutes
    MAX_TASK_SIZE = 100 * 1024 * 1024  # 100MB
    
    # Storage settings
    STORAGE_PATH = "/tmp/taskworker"
    MAX_STORAGE_SIZE = 10 * 1024 * 1024 * 1024  # 10GB
```

## API Usage Examples

### Python Client Usage

```python
import grpc
from taskworker.protos import taskworker_pb2, taskworker_pb2_grpc

# Connect to TaskWorker service
channel = grpc.insecure_channel('localhost:50053')
stub = taskworker_pb2_grpc.TaskWorkerServiceStub(channel)

# Execute a Python task
execute_request = taskworker_pb2.ExecuteTaskRequest(
    task_id="task_001",
    task_type="python",
    task_data=b"import time; time.sleep(2); result = 42"
)

response = stub.ExecuteTask(execute_request)
print(f"Task submitted: {response.success}")
```

## Performance and Scaling

| Metric | Value | Notes |
|--------|-------|-------|
| Task Throughput | 100-1000/sec | Depends on task complexity |
| Concurrent Tasks | 4-16 | Configurable per node |
| Memory Overhead | 50-100MB | Base consumption |
| Startup Time | < 2 seconds | Service initialization |

## Security Features

- Input validation and sanitization
- Resource isolation using containers
- Authentication and access control
- Rate limiting and DoS protection

## Deployment Options

### Docker Deployment

```dockerfile
FROM python:3.9-slim
WORKDIR /app
COPY requirements.txt .
RUN pip install -r requirements.txt
COPY taskworker/ ./taskworker/
EXPOSE 50053
CMD ["python", "taskworker/rpc_service.py"]
```

---

**Module Version**: v1.0  
**Last Updated**: January 2024  
**Status**: Production Ready

# Node Pool Module

## Overview

The Node Pool is the core service of HiveMind, responsible for node management, task scheduling, user authentication, and resource coordination. It acts as the central hub that connects all components of the distributed computing platform.

## Architecture

```
┌─────────────────────────────────────────┐
│              Node Pool Service           │
├─────────────────────────────────────────┤
│ ┌─────────────┐ ┌─────────────────────┐ │
│ │User Service │ │Node Manager Service │ │
│ │             │ │                     │ │
│ │• Auth       │ │• Registration       │ │
│ │• Sessions   │ │• Status Tracking    │ │
│ │• Profiles   │ │• Resource Monitor   │ │
│ └─────────────┘ └─────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │     Master Node Service             │ │
│ │                                     │ │
│ │• Task Scheduling                    │ │
│ │• Reward System                      │ │
│ │• System Monitoring                  │ │
│ └─────────────────────────────────────┘ │
├─────────────────────────────────────────┤
│          Data Storage Layer             │
│ ┌─────────┐ ┌─────────┐ ┌─────────────┐ │
│ │ SQLite  │ │  Redis  │ │File Storage │ │
│ │ Users   │ │ State   │ │   Tasks     │ │
│ └─────────┘ └─────────┘ └─────────────┘ │
└─────────────────────────────────────────┘
```

## Core Components

### 1. User Service (`user_service.py`)

**Responsibilities**:
- User registration and authentication
- Session management
- User profile management
- Authorization and permissions

**Key Features**:
- Password hashing with salt
- Session token generation
- User role management
- Account validation

**gRPC Methods**:
```python
class UserServiceServicer:
    def RegisterUser(self, request, context)
    def LoginUser(self, request, context)
    def LogoutUser(self, request, context)
    def GetUserInfo(self, request, context)
    def UpdateUserProfile(self, request, context)
```

### 2. Node Manager Service (`node_manager_service.py`)

**Responsibilities**:
- Worker node registration and management
- Node status monitoring
- Resource tracking
- Node health checks

**Key Features**:
- Dynamic node discovery
- Resource capacity tracking
- Node performance metrics
- Automatic node cleanup

**gRPC Methods**:
```python
class NodeManagerServiceServicer:
    def RegisterNode(self, request, context)
    def UnregisterNode(self, request, context)
    def UpdateNodeStatus(self, request, context)
    def GetNodeInfo(self, request, context)
    def ListNodes(self, request, context)
```

### 3. Master Node Service (`master_node_service.py`)

**Responsibilities**:
- Task scheduling and distribution
- Reward calculation and distribution
- System monitoring
- Performance analytics

**Key Features**:
- Intelligent task scheduling
- Load balancing
- Reward system
- System health monitoring

**gRPC Methods**:
```python
class MasterNodeServiceServicer:
    def SubmitTask(self, request, context)
    def GetTaskStatus(self, request, context)
    def CancelTask(self, request, context)
    def GetTaskResult(self, request, context)
    def GetSystemStats(self, request, context)
```

## Configuration

### Server Configuration (`config.py`)

```python
class Config:
    # gRPC Server Settings
    GRPC_PORT = 50051
    GRPC_HOST = "0.0.0.0"
    MAX_WORKERS = 20
    
    # Message Size Limits
    MAX_MESSAGE_SIZE = 100 * 1024 * 1024  # 100MB
    MAX_FRAME_SIZE = 16 * 1024 * 1024     # 16MB
    
    # Storage Paths
    DATABASE_PATH = "users.db"
    TASK_STORAGE_PATH = "/mnt/myusb/hivemind/task_storage"
    
    # Redis Configuration
    REDIS_HOST = "localhost"
    REDIS_PORT = 6379
    REDIS_DB = 0
    
    # Security Settings
    SESSION_TIMEOUT = 3600  # 1 hour
    PASSWORD_SALT_LENGTH = 32
```

### gRPC Server Options

The Node Pool server is configured with optimized gRPC options for high-performance operation:

```python
options = [
    ('grpc.keepalive_time_ms', 10000),              # Keepalive every 10s
    ('grpc.keepalive_timeout_ms', 5000),            # Keepalive timeout 5s
    ('grpc.keepalive_permit_without_calls', True),  # Allow keepalive without calls
    ('grpc.http2.max_pings_without_data', 0),       # Unlimited pings
    ('grpc.http2.min_time_between_pings_ms', 10000), # Min ping interval
    ('grpc.max_receive_message_length', 100 * 1024 * 1024), # 100MB max receive
    ('grpc.max_send_message_length', 100 * 1024 * 1024),    # 100MB max send
    ('grpc.http2.max_frame_size', 16 * 1024 * 1024),        # 16MB max frame
]
```

## Data Models

### User Data Model

```python
@dataclass
class User:
    user_id: str
    username: str
    email: str
    password_hash: str
    salt: str
    created_at: datetime
    last_login: datetime
    role: str = "user"
    is_active: bool = True
```

### Node Data Model

```python
@dataclass
class NodeInfo:
    node_id: str
    node_type: str  # "worker" or "master"
    hostname: str
    port: int
    capabilities: Dict[str, Any]
    status: str     # "online", "offline", "busy"
    last_heartbeat: datetime
    resource_usage: Dict[str, float]
```

### Task Data Model

```python
@dataclass
class Task:
    task_id: str
    user_id: str
    task_type: str
    task_data: bytes
    status: str     # "pending", "running", "completed", "failed"
    created_at: datetime
    started_at: Optional[datetime]
    completed_at: Optional[datetime]
    assigned_node: Optional[str]
    result: Optional[bytes]
    error_message: Optional[str]
```

## API Usage Examples

### User Registration and Login

```python
import grpc
import nodepool_pb2
import nodepool_pb2_grpc

# Connect to Node Pool
channel = grpc.insecure_channel('localhost:50051')
user_stub = nodepool_pb2_grpc.UserServiceStub(channel)

# Register new user
register_request = nodepool_pb2.RegisterUserRequest(
    username="john_doe",
    password="secure_password123",
    email="john@example.com"
)
register_response = user_stub.RegisterUser(register_request)

if register_response.success:
    print(f"User registered successfully: {register_response.user_id}")
    
    # Login user
    login_request = nodepool_pb2.LoginUserRequest(
        username="john_doe",
        password="secure_password123"
    )
    login_response = user_stub.LoginUser(login_request)
    
    if login_response.success:
        session_token = login_response.session_token
        print(f"Login successful, session token: {session_token}")
```

### Node Registration

```python
# Connect to Node Pool
node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(channel)

# Register worker node
register_node_request = nodepool_pb2.RegisterNodeRequest(
    node_id="worker-001",
    node_type="worker",
    hostname="192.168.1.100",
    port=50052,
    capabilities=nodepool_pb2.NodeCapabilities(
        cpu_cores=8,
        memory_mb=16384,
        has_gpu=True,
        supported_tasks=["python", "docker", "ml_inference"]
    )
)

register_node_response = node_stub.RegisterNode(register_node_request)
if register_node_response.success:
    print(f"Node registered: {register_node_response.assigned_id}")
```

### Task Submission

```python
# Connect to Master Node Service
master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(channel)

# Submit task
task_request = nodepool_pb2.SubmitTaskRequest(
    user_id="user_123",
    task_type="python",
    task_data=b"print('Hello, HiveMind!')",
    requirements=nodepool_pb2.TaskRequirements(
        cpu_cores=2,
        memory_mb=1024,
        timeout_seconds=300
    ),
    priority=nodepool_pb2.TaskPriority.NORMAL
)

task_response = master_stub.SubmitTask(task_request)
if task_response.success:
    task_id = task_response.task_id
    print(f"Task submitted: {task_id}")
    
    # Check task status
    status_request = nodepool_pb2.GetTaskStatusRequest(
        task_id=task_id,
        user_id="user_123"
    )
    status_response = master_stub.GetTaskStatus(status_request)
    print(f"Task status: {status_response.status}")
```

## Database Schema

### Users Table (SQLite)

```sql
CREATE TABLE users (
    user_id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    salt TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_login TIMESTAMP,
    role TEXT DEFAULT 'user',
    is_active BOOLEAN DEFAULT 1
);
```

### Redis Data Structures

```
# Node information
worker_nodes:
  HSET worker:node_id {
    "status": "online",
    "last_heartbeat": "2024-01-15T10:30:00Z",
    "cpu_usage": "45.2",
    "memory_usage": "60.8",
    "task_count": "3"
  }

# Active sessions
user_sessions:
  HSET session:token {
    "user_id": "user_123",
    "created_at": "2024-01-15T10:00:00Z",
    "expires_at": "2024-01-15T11:00:00Z"
  }

# Task queue
task_queue:
  LPUSH pending_tasks "task_id_1"
  LPUSH pending_tasks "task_id_2"
  
# Task information
task_info:
  HSET task:task_id {
    "status": "running",
    "assigned_node": "worker-001",
    "started_at": "2024-01-15T10:25:00Z"
  }
```

## Performance Characteristics

### Throughput Metrics

| Operation | Throughput | Latency |
|-----------|------------|---------|
| User Login | 1000/sec | < 10ms |
| Node Registration | 100/sec | < 50ms |
| Task Submission | 500/sec | < 20ms |
| Status Queries | 2000/sec | < 5ms |

### Resource Usage

- **Memory**: 256MB base + 1MB per active node
- **CPU**: Low overhead, <5% on 4-core system
- **Network**: Varies by task data size
- **Storage**: SQLite database + Redis memory

## Monitoring and Metrics

### Health Checks

```python
# Health check endpoint
def health_check():
    checks = {
        "database": check_database_connection(),
        "redis": check_redis_connection(),
        "disk_space": check_disk_space(),
        "memory_usage": get_memory_usage()
    }
    return checks
```

### Key Metrics

```python
# Performance metrics
metrics = {
    "active_nodes": get_active_node_count(),
    "pending_tasks": get_pending_task_count(),
    "completed_tasks_24h": get_completed_tasks_count(24),
    "average_task_duration": get_average_task_duration(),
    "system_uptime": get_system_uptime()
}
```

## Security Features

### Authentication

- Password hashing using PBKDF2 with salt
- Session token-based authentication
- Token expiration and renewal
- Role-based access control

### Authorization

- User role verification
- Resource access control
- API endpoint protection
- Rate limiting (configurable)

### Data Protection

- Encrypted password storage
- Secure session management
- Input validation and sanitization
- SQL injection prevention

## Deployment Considerations

### Single Instance Deployment

```bash
# Start Node Pool service
cd node_pool
python node_pool_server.py
```

### High Availability Deployment

```yaml
# docker-compose.yml
version: '3.8'
services:
  nodepool-primary:
    build: ./node_pool
    ports:
      - "50051:50051"
    environment:
      - REDIS_HOST=redis
      - DB_PATH=/data/users.db
    volumes:
      - nodepool_data:/data
      
  nodepool-backup:
    build: ./node_pool
    ports:
      - "50052:50051"
    environment:
      - REDIS_HOST=redis
      - DB_PATH=/data/users.db
    volumes:
      - nodepool_data:/data
      
  redis:
    image: redis:6.2-alpine
    volumes:
      - redis_data:/data
      
volumes:
  nodepool_data:
  redis_data:
```

## Troubleshooting

### Common Issues

1. **Port Already in Use**
   ```bash
   # Check what's using port 50051
   netstat -tlnp | grep :50051
   # Kill the process or change port
   ```

2. **Redis Connection Failed**
   ```bash
   # Check Redis status
   redis-cli ping
   # Restart Redis if needed
   sudo systemctl restart redis
   ```

3. **Database Lock**
   ```bash
   # Check for stale lock files
   ls -la users.db*
   # Remove lock files if safe
   rm users.db-wal users.db-shm
   ```

### Performance Tuning

1. **Increase Worker Threads**
   ```python
   # In node_pool_server.py
   server = grpc.server(
       futures.ThreadPoolExecutor(max_workers=50),  # Increase from 20
       options=options
   )
   ```

2. **Optimize Redis Memory**
   ```conf
   # In redis.conf
   maxmemory 2gb
   maxmemory-policy allkeys-lru
   ```

3. **Database Optimization**
   ```sql
   -- Add indexes for common queries
   CREATE INDEX idx_users_username ON users(username);
   CREATE INDEX idx_users_email ON users(email);
   ```

---

**Module Version**: v1.0  
**Last Updated**: January 2024  
**Status**: Production Ready

**Dependencies**: grpc, redis, sqlite3, concurrent.futures  
**Python Version**: 3.8+

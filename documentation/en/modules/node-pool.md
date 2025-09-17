# Node Pool Module

## Overview

The Node Pool is the central resource scheduling system of HiveMind distributed computing platform, featuring a multi-level trust system for intelligent node management, task allocation, user authentication, and dynamic resource coordination. Built on gRPC for high-performance distributed services.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Node Pool Service                         │
├─────────────────────────────────────────────────────────────┤
│  User Service     │  Node Manager   │  Master Node Service │
├─────────────────────────────────────────────────────────────┤
│             Multi-Level Trust System & Dynamic Resource     │
│                          Allocation                         │
├─────────────────────────────────────────────────────────────┤
│  SQLite Database │  Redis Cache   │  gRPC Services          │
└─────────────────────────────────────────────────────────────┘
        │                    │                    │
   High Trust Nodes      Normal Trust Nodes   Low Trust Nodes
   (Credit ≥ 100)        (Credit 50-99)      (Credit < 50)
```

## Core Components

### 1. Node Pool Server (`node_pool_server.py`)
- **Function**: Main gRPC server with multi-service integration
- **Port**: 50051 (configurable)
- **Protocol**: gRPC with Protocol Buffers
- **Features**: 
  - Large file transfer support (100MB)
  - 20 concurrent worker threads
  - Advanced keep-alive configuration

### 2. Node Manager (`node_manager.py`)
- **Function**: Intelligent node lifecycle management
- **Core Features**:
  - **Multi-Level Trust System**: Node grouping based on credit scores
  - **Dynamic Resource Tracking**: Real-time monitoring of total and available resources
  - **Multi-Task Support**: Parallel task execution on single nodes
  - **Docker Awareness**: Trust level adjustment based on Docker status
  - **Geographic Awareness**: Location-based node prioritization
  - **Load Balancing**: Intelligent task distribution algorithm

### 3. User Service (`user_service.py`)
- **Function**: JWT-based user authentication service
- **Responsibilities**:
  - User registration and login
  - JWT Token management
  - CPT token transfers
  - Balance queries

### 4. Master Node Service (`master_node_service.py`)
- **Function**: Task scheduling and management
- **Responsibilities**:
  - Task upload and distribution
  - Execution status monitoring
  - Result collection
  - Task termination control

### 5. Configuration (`config.py`)
- **Function**: Unified configuration management
- **Support**: Environment variables and .env files

## Trust Level System

### Trust Level Classification
```python
# High Trust Level
- Credit Score >= 100
- Docker Status: enabled
- Priority: Highest

# Normal Trust Level  
- Credit Score 50-99
- Docker Status: enabled
- Priority: Medium

# Low Trust Level
- Credit Score < 50 or Docker disabled
- Restriction: Only accepts tasks from high-trust users
- Priority: Lowest
```

### Task Allocation Algorithm
1. **Trust Group Priority**: High Trust → Normal Trust → Low Trust
2. **Resource Matching**: CPU, Memory, GPU requirement checks
3. **Load Balancing**: Prioritize nodes with lower load
4. **Docker Compatibility**: Non-Docker nodes require user trust score > 50
5. **Geographic Location**: Support for location preferences

## gRPC Service Definitions

### UserService
```protobuf
service UserService {
    rpc Login(LoginRequest) returns (LoginResponse);
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Transfer(TransferRequest) returns (TransferResponse);
    rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
}
```

**Key Message Types**:
```protobuf
message LoginRequest {
    string username = 1;
    string password = 2;
}

message LoginResponse {
    bool success = 1;
    string message = 2;
    string token = 3;
}

message GetBalanceRequest {
    string username = 1;
    string token = 2;
}

message TransferRequest {
    string token = 1;
    string receiver_username = 2;
    int64 amount = 3;
}
```

### NodeManagerService
```protobuf
service NodeManagerService {
    rpc RegisterWorkerNode(RegisterWorkerNodeRequest) returns (StatusResponse);
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc ReportStatus(ReportStatusRequest) returns (StatusResponse);
    rpc GetNodeList(GetNodeListRequest) returns (GetNodeListResponse);
}
```

**Node Registration Messages**:
```protobuf
message RegisterWorkerNodeRequest {
    string node_id = 1;        // Username
    string hostname = 2;       // IP Address
    int32 cpu_cores = 3;
    int32 memory_gb = 4;
    int32 cpu_score = 5;
    int32 gpu_score = 6;
    int32 gpu_memory_gb = 7;
    string location = 8;
    int32 port = 9;
    string gpu_name = 12;
    string docker_status = 13;
}

message WorkerNodeInfo {
    string node_id = 1;
    string hostname = 2;
    int32 cpu_cores = 3;
    int32 memory_gb = 4;
    string status = 5;
    double last_heartbeat = 6;
    int32 cpu_score = 7;
    int32 gpu_score = 8;
    int32 gpu_memory_gb = 9;
    string location = 10;
    int32 port = 11;
    string gpu_name = 12;
}
```

### MasterNodeService
```protobuf
service MasterNodeService {
    rpc UploadTask(UploadTaskRequest) returns (UploadTaskResponse);
    rpc PollTaskStatus(PollTaskStatusRequest) returns (PollTaskStatusResponse);
    rpc StoreOutput(StoreOutputRequest) returns (StatusResponse);
    rpc StoreResult(StoreResultRequest) returns (StatusResponse);
    rpc GetTaskResult(GetTaskResultRequest) returns (GetTaskResultResponse);
    rpc TaskCompleted(TaskCompletedRequest) returns (StatusResponse);
    rpc StoreLogs(StoreLogsRequest) returns (StatusResponse);
    rpc GetTaskLogs(GetTaskLogsRequest) returns (GetTaskLogsResponse);
    rpc GetAllTasks(GetAllTasksRequest) returns (GetAllTasksResponse);
    rpc StopTask(StopTaskRequest) returns (StopTaskResponse);
    rpc ReturnTaskResult(ReturnTaskResultRequest) returns (ReturnTaskResultResponse);
}
```

**Task-Related Messages**:
```protobuf
message UploadTaskRequest {
    string task_id = 1;
    bytes task_zip = 2;
    int32 memory_gb = 3;
    int32 cpu_score = 4;
    int32 gpu_score = 5;
    int32 gpu_memory_gb = 6;
    string location = 7;
    string gpu_name = 8;
    string user_id = 9;
}

message TaskStatus {
    string task_id = 1;
    string status = 2;
    string created_at = 3;
    string updated_at = 4;
    string assigned_node = 5;
}

message PollTaskStatusResponse {
    string task_id = 1;
    string status = 2;
    repeated string output = 3;
    string message = 4;
}
```

### WorkerNodeService
```protobuf
service WorkerNodeService {
    rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
    rpc ReportOutput(ReportOutputRequest) returns (StatusResponse);
    rpc ReportRunningStatus(RunningStatusRequest) returns (RunningStatusResponse);
    rpc StopTaskExecution(StopTaskExecutionRequest) returns (StopTaskExecutionResponse);
}
```

**Worker Node Messages**:
```protobuf
message ExecuteTaskRequest {
    string node_id = 1;
    string task_id = 2;
    bytes task_zip = 3;
    int32 cpu = 4;
    int32 gpu = 5;
    int32 memory_gb = 6;
    int32 gpu_memory_gb = 7;
}

message RunningStatusRequest {
    string node_id = 1;
    string task_id = 2;
    int32 cpu_usage = 3;      // CPU usage percentage
    int32 memory_usage = 4;   // Memory usage percentage
    int32 gpu_usage = 5;      // GPU usage percentage
    int32 gpu_memory_usage = 6; // GPU memory usage percentage
}

message RunningStatusResponse {
    bool success = 1;
    string message = 2;
    int64 cpt_reward = 3;
}
```

## Data Storage Architecture

### SQLite User Database
- **File**: `users.db`
- **Function**: User authentication, credit scores, token balances

### Redis Node State Cache
- **Purpose**: Real-time node status, resource tracking
- **Key-Value Structure**:
  ```
  node:{node_id} -> {
    "hostname": "worker01",
    "status": "Idle",
    "cpu_score": "1000",
    "available_cpu_score": "800",
    "current_tasks": "2",
    "trust_level": "high",
    "docker_status": "enabled",
    ...
  }
  ```

## Resource Management System

### Resource Type Definitions
```python
# CPU Score: Processor computing capability score
# Memory: Memory capacity in GB
# GPU Score: Graphics processor computing capability score
# GPU Memory: Graphics memory capacity in GB
```

### Dynamic Resource Allocation
```python
def allocate_node_resources(node_id, task_id, cpu_score, memory_gb, gpu_score, gpu_memory_gb):
    """Allocate node resources to task"""
    # Check available resources
    # Deduct allocated resources
    # Update task list
    # Record allocation status
```

### Resource Release Mechanism
```python
def release_node_resources(node_id, task_id, cpu_score, memory_gb, gpu_score, gpu_memory_gb):
    """Release node resources"""
    # Return resources to available pool
    # Remove task record
    # Update node status
```

## Deployment and Configuration

### Environment Variables
```bash
# gRPC Service Configuration
GRPC_SERVER_HOST=0.0.0.0
GRPC_SERVER_PORT=50051

# Redis Configuration
REDIS_HOST=localhost
REDIS_PORT=6379
REDIS_DB=0

# JWT Authentication Configuration
JWT_SECRET_KEY=your-secret-key
TOKEN_EXPIRATION_HOURS=24

# Storage Configuration
TASK_STORAGE_PATH=/mnt/myusb/hivemind/task_storage
MAX_FILE_SIZE=10485760  # 10MB

# Database Configuration
DB_PATH=./users.db
```

### Starting the Service
```bash
cd node_pool
pip install -r requirements.txt
python node_pool_server.py
```

## Monitoring and Logging

### Key Metrics Monitoring
- **Node Metrics**: Active nodes, trust level distribution
- **Task Metrics**: Pending tasks, completion rate, failure rate
- **Resource Metrics**: Total resources, available resources, utilization rate
- **System Metrics**: gRPC response time, error rate

### Log Classification
```python
# Node management logs
logging.info(f"Node {node_id} (GPU: {gpu_name}, Docker: {docker_status}) registered successfully")

# Task allocation logs  
logging.info(f"Task {task_id} assigned to node {node_id}, trust level: {trust_level}")

# Resource management logs
logging.info(f"Node {node_id} resource allocation: CPU-{cpu_score}, Memory-{memory_gb}GB")
```

## API Usage Examples

### Node Registration Example
```python
import grpc
import nodepool_pb2
import nodepool_pb2_grpc

channel = grpc.insecure_channel('localhost:50051')
stub = nodepool_pb2_grpc.NodeManagerServiceStub(channel)

request = nodepool_pb2.RegisterWorkerNodeRequest(
    node_id="worker-001",        # Username
    hostname="192.168.1.100",    # IP Address
    cpu_cores=8,
    memory_gb=16,
    cpu_score=1000,
    gpu_score=2000,
    gpu_memory_gb=8,
    location="Asia/Taipei",
    port=50052,
    gpu_name="RTX 4090",
    docker_status="enabled"
)

response = stub.RegisterWorkerNode(request)
print(f"Registration result: {response.message}")
```

### User Login Example
```python
user_stub = nodepool_pb2_grpc.UserServiceStub(channel)

# User login
login_request = nodepool_pb2.LoginRequest(
    username="user123",
    password="password123"
)

login_response = user_stub.Login(login_request)
if login_response.success:
    token = login_response.token
    print(f"Login successful, Token: {token}")
    
    # Check balance
    balance_request = nodepool_pb2.GetBalanceRequest(
        username="user123",
        token=token
    )
    
    balance_response = user_stub.GetBalance(balance_request)
    if balance_response.success:
        print(f"Account balance: {balance_response.balance} CPT")
```

### Task Upload Example
```python
master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(channel)

with open("task.zip", "rb") as f:
    task_data = f.read()

request = nodepool_pb2.UploadTaskRequest(
    task_id="task-001",
    task_zip=task_data,
    memory_gb=4,
    cpu_score=500,
    gpu_score=1000,
    gpu_memory_gb=4,
    location="Any",
    gpu_name="Any",
    user_id="user123"
)

response = master_stub.UploadTask(request)
print(f"Task upload: {response.message}")

# Poll task status
status_request = nodepool_pb2.PollTaskStatusRequest(task_id="task-001")
status_response = master_stub.PollTaskStatus(status_request)
print(f"Task status: {status_response.status}")
print(f"Output: {status_response.output}")
```

### Worker Node Status Report Example
```python
worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(channel)

# Report running status
status_request = nodepool_pb2.RunningStatusRequest(
    node_id="worker-001",
    task_id="task-001",
    cpu_usage=75,      # CPU usage 75%
    memory_usage=60,   # Memory usage 60%
    gpu_usage=85,      # GPU usage 85%
    gpu_memory_usage=70  # GPU memory usage 70%
)

status_response = worker_stub.ReportRunningStatus(status_request)
if status_response.success:
    print(f"Status report successful, CPT reward earned: {status_response.cpt_reward}")
```

## Troubleshooting

### 1. gRPC Connection Issues
```bash
# Check service status
netstat -an | grep 50051

# Test connection
grpcurl -plaintext localhost:50051 nodepool.NodeManagerService/HealthCheck
```

### 2. Redis Connection Failure
```bash
# Check Redis service
redis-cli ping

# Restart Redis
sudo systemctl restart redis-server
```

### 3. Node Registration Failure
```python
# Check node information
node_info = node_manager.get_node_info(node_id)
if not node_info:
    print(f"Node {node_id} does not exist or is not registered")
```

### 4. Task Allocation Failure
```python
# Check available nodes
available_nodes = node_manager.get_available_nodes(
    memory_gb_req=4, cpu_score_req=500, 
    gpu_score_req=1000, gpu_memory_gb_req=4,
    location_req="Any", gpu_name_req="Any",
    user_trust_score=100
)
print(f"Available nodes: {len(available_nodes)}")
```

## Performance Optimization

### gRPC Server Optimization
```python
options = [
    ('grpc.keepalive_time_ms', 10000),
    ('grpc.keepalive_timeout_ms', 5000),
    ('grpc.max_receive_message_length', 100 * 1024 * 1024),  # 100MB
    ('grpc.max_send_message_length', 100 * 1024 * 1024),     # 100MB
]

server = grpc.server(
    futures.ThreadPoolExecutor(max_workers=20),
    options=options
)
```

### Redis Memory Optimization
```bash
# Set Redis memory limit
maxmemory 2gb
maxmemory-policy allkeys-lru

# Monitor memory usage
redis-cli info memory
```

### Database Index Optimization
```sql
-- Create indexes for key fields (if using SQL queries)
CREATE INDEX idx_user_username ON users(username);
CREATE INDEX idx_user_credit_score ON users(credit_score);
```

## Security

### JWT Authentication Mechanism
- **Key Management**: Secure storage in environment variables
- **Expiration Time**: Configurable token validity period
- **Permission Control**: API access based on user identity

### gRPC Security
- **TLS Encryption**: Support for HTTPS/gRPC-TLS
- **Message Verification**: Prevent forged requests
- **Rate Limiting**: DDoS attack prevention

### Data Protection
- **Password Encryption**: bcrypt hash encryption
- **Sensitive Data**: Encrypted storage of confidential information
- **Access Logging**: Record all API calls

---

**Related Documentation**:
- [API Documentation](../api.md)
- [Deployment Guide](../deployment.md)
- [Troubleshooting](../troubleshooting.md)
- [Developer Guide](../developer.md)

# HiveMind API Documentation

## Overview

HiveMind uses gRPC protocol for inter-service communication, with all API interfaces based on Protocol Buffers definitions. This document provides detailed specifications for all service APIs.

## Service Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Master Node   │    │   Node Pool     │    │  Worker Node    │
│                 │    │                 │    │                 │
│ Port: 50051     │◄──►│ Port: 50051     │◄──►│ Dynamic Port    │
│ User Service    │    │ Node Management │    │ Task Execution  │
│ Task Management │    │ Task Scheduling │    │ Status Reporting│
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## gRPC Service Definitions

Based on the actual `worker/nodepool.proto` file, HiveMind contains four main gRPC services:

### UserService

**File**: `worker/nodepool.proto`

```protobuf
service UserService {
    rpc Login(LoginRequest) returns (LoginResponse);
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Transfer(TransferRequest) returns (TransferResponse);
    rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
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

### WorkerNodeService

```protobuf
service WorkerNodeService {
    rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
    rpc ReportOutput(ReportOutputRequest) returns (StatusResponse);
    rpc ReportRunningStatus(RunningStatusRequest) returns (RunningStatusResponse);
    rpc StopTaskExecution(StopTaskExecutionRequest) returns (StopTaskExecutionResponse);
}
```

## User Management API

### User Registration

**Method**: `Register`
**Service**: `UserService`

**Request Parameters**:
```protobuf
message RegisterRequest {
    string username = 1;     // Username
    string password = 2;     // Password
}
```

**Response Parameters**:
```protobuf
message RegisterResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

### User Login

**Method**: `Login`
**Service**: `UserService`

**Request Parameters**:
```protobuf
message LoginRequest {
    string username = 1;     // Username
    string password = 2;     // Password
}
```

**Response Parameters**:
```protobuf
message LoginResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
    string token = 3;        // JWT authentication token
}
```

### Balance Query

**Method**: `GetBalance`
**Service**: `UserService`

**Request Parameters**:
```protobuf
message GetBalanceRequest {
    string username = 1;     // Username
    string token = 2;        // Authentication token
}
```

**Response Parameters**:
```protobuf
message GetBalanceResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
    int64 balance = 3;       // User balance
}
```

### Transfer Transaction

**Method**: `Transfer`
**Service**: `UserService`

**Request Parameters**:
```protobuf
message TransferRequest {
    string token = 1;            // Authentication token
    string receiver_username = 2; // Receiver username
    int64 amount = 3;           // Transfer amount
}
```

**Response Parameters**:
```protobuf
message TransferResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

## Node Management API

### Worker Node Registration

**Method**: `RegisterWorkerNode`
**Service**: `NodeManagerService`

**Request Parameters**:
```protobuf
message RegisterWorkerNodeRequest {
    string node_id = 1;          // Node ID (username)
    string hostname = 2;         // IP address
    int32 cpu_cores = 3;         // CPU cores
    int32 memory_gb = 4;         // Memory capacity (GB)
    int32 cpu_score = 5;         // CPU performance score
    int32 gpu_score = 6;         // GPU performance score
    int32 gpu_memory_gb = 7;     // GPU memory capacity (GB)
    string location = 8;         // Geographic location
    int32 port = 9;             // Service port
    string gpu_name = 10;        // GPU name
    int32 trust_level = 11;      // Trust level
    double last_heartbeat = 12;   // Last heartbeat time
    string docker_status = 13;    // Docker status
}
```

**Response Parameters**:
```protobuf
message StatusResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

### Health Check

**Method**: `HealthCheck`
**Service**: `NodeManagerService`

**Request Parameters**:
```protobuf
message HealthCheckRequest {
    string node_id = 1;      // Node ID
}
```

**Response Parameters**:
```protobuf
message HealthCheckResponse {
    string status = 1;       // Health status
    string message = 2;      // Status message
}
```

### Status Report

**Method**: `ReportStatus`
**Service**: `NodeManagerService`

**Request Parameters**:
```protobuf
message ReportStatusRequest {
    string node_id = 1;      // Node ID
    string status = 2;       // Node status
}
```

### Get Node List

**Method**: `GetNodeList`
**Service**: `NodeManagerService`

**Request Parameters**:
```protobuf
message GetNodeListRequest {
    // Empty request, get all nodes
}
```

**Response Parameters**:
```protobuf
message GetNodeListResponse {
    repeated WorkerNodeInfo nodes = 1; // Node list
}

message WorkerNodeInfo {
    string node_id = 1;          // Node ID
    string hostname = 2;         // IP address
    int32 cpu_cores = 3;         // CPU cores
    int32 memory_gb = 4;         // Memory capacity
    int32 cpu_score = 5;         // CPU performance score
    int32 gpu_score = 6;         // GPU performance score
    int32 gpu_memory_gb = 7;     // GPU memory capacity
    string location = 8;         // Geographic location
    int32 port = 9;             // Service port
    string gpu_name = 10;        // GPU name
    int32 trust_level = 11;      // Trust level
    double last_heartbeat = 12;   // Last heartbeat time
    string docker_status = 13;    // Docker status
}
```

## Task Management API

### Upload Task

**Method**: `UploadTask`
**Service**: `MasterNodeService`

**Request Parameters**:
```protobuf
message UploadTaskRequest {
    string task_id = 1;      // Task ID
    bytes task_data = 2;     // Task data
    string user_id = 3;      // User ID
}
```

**Response Parameters**:
```protobuf
message UploadTaskResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

### Query Task Status

**Method**: `PollTaskStatus`
**Service**: `MasterNodeService`

**Request Parameters**:
```protobuf
message PollTaskStatusRequest {
    string task_id = 1;      // Task ID
}
```

**Response Parameters**:
```protobuf
message PollTaskStatusResponse {
    bool success = 1;            // Success flag
    string message = 2;          // Response message
    string status = 3;           // Task status
    repeated string output = 4;   // Output content
}
```

### Get Task Result

**Method**: `GetTaskResult`
**Service**: `MasterNodeService`

**Request Parameters**:
```protobuf
message GetTaskResultRequest {
    string task_id = 1;      // Task ID
}
```

**Response Parameters**:
```protobuf
message GetTaskResultResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
    bytes result_data = 3;   // Result data
}
```

### Stop Task

**Method**: `StopTask`
**Service**: `MasterNodeService`

**Request Parameters**:
```protobuf
message StopTaskRequest {
    string task_id = 1;      // Task ID
}
```

**Response Parameters**:
```protobuf
message StopTaskResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

### Get All Tasks

**Method**: `GetAllTasks`
**Service**: `MasterNodeService`

**Request Parameters**:
```protobuf
message GetAllTasksRequest {
    // Empty request, get all tasks
}
```

**Response Parameters**:
```protobuf
message GetAllTasksResponse {
    repeated TaskStatus tasks = 1; // Task list
}

message TaskStatus {
    string task_id = 1;       // Task ID
    string status = 2;        // Task status
    string assigned_node = 3; // Assigned node
    double created_at = 4;    // Creation time
    double started_at = 5;    // Start time
    double completed_at = 6;  // Completion time
    string user_id = 7;       // User ID
}
```

## Worker Node API

### Execute Task

**Method**: `ExecuteTask`
**Service**: `WorkerNodeService`

**Request Parameters**:
```protobuf
message ExecuteTaskRequest {
    string task_id = 1;      // Task ID
    bytes task_data = 2;     // Task data
}
```

**Response Parameters**:
```protobuf
message ExecuteTaskResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

### Report Running Status

**Method**: `ReportRunningStatus`
**Service**: `WorkerNodeService`

**Request Parameters**:
```protobuf
message RunningStatusRequest {
    string node_id = 1;          // Node ID
    string task_id = 2;          // Task ID
    int32 cpu_usage = 3;         // CPU usage
    int32 memory_usage = 4;      // Memory usage
    int32 gpu_usage = 5;         // GPU usage
    int32 gpu_memory_usage = 6;  // GPU memory usage
}
```

**Response Parameters**:
```protobuf
message RunningStatusResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
    int64 cpt_reward = 3;    // CPT reward
}
```

### Stop Task Execution

**Method**: `StopTaskExecution`
**Service**: `WorkerNodeService`

**Request Parameters**:
```protobuf
message StopTaskExecutionRequest {
    string task_id = 1;      // Task ID
}
```

**Response Parameters**:
```protobuf
message StopTaskExecutionResponse {
    bool success = 1;        // Success flag
    string message = 2;      // Response message
}
```

## TaskWorker gRPC API

### Service Definition

**File**: `taskworker/protos/taskworker.proto`

```protobuf
service TaskWorkerService {
  rpc ExecuteTask(ExecuteTaskRequest) returns (ExecuteTaskResponse);
  rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
  rpc CancelTask(CancelTaskRequest) returns (CancelTaskResponse);
  rpc GetSystemInfo(GetSystemInfoRequest) returns (GetSystemInfoResponse);
}
```

### Execute Task

**Method**: `ExecuteTask`

**Request Parameters**:
```protobuf
message ExecuteTaskRequest {
  string task_id = 1;       // Task ID
  string task_type = 2;     // Task type
  bytes task_data = 3;      // Task data
  map<string, string> parameters = 4; // Task parameters
}
```

## Data Structure Definitions

### User Information

```protobuf
message UserInfo {
  string user_id = 1;       // User ID
  string username = 2;      // Username
  string email = 3;         // Email
  int64 created_at = 4;     // Creation time
  UserRole role = 5;        // User role
}
```

### Node Capabilities

```protobuf
message NodeCapabilities {
  int32 cpu_cores = 1;      // CPU cores
  int64 memory_mb = 2;      // Memory capacity (MB)
  bool has_gpu = 3;         // Has GPU
  repeated string supported_tasks = 4; // Supported task types
}
```

### Resource Usage

```protobuf
message ResourceUsage {
  float cpu_percent = 1;    // CPU usage percentage
  float memory_percent = 2; // Memory usage percentage
  float disk_percent = 3;   // Disk usage percentage
  float network_mbps = 4;   // Network usage (Mbps)
}
```

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| 0 | SUCCESS | Operation successful |
| 1001 | USER_NOT_FOUND | User not found |
| 1002 | INVALID_PASSWORD | Invalid password |
| 1003 | USER_EXISTS | User already exists |
| 1004 | INVALID_SESSION | Invalid session |
| 2001 | NODE_NOT_FOUND | Node not found |
| 2002 | NODE_OFFLINE | Node offline |
| 2003 | INSUFFICIENT_RESOURCES | Insufficient resources |
| 3001 | TASK_NOT_FOUND | Task not found |
| 3002 | TASK_FAILED | Task execution failed |
| 3003 | TASK_CANCELLED | Task cancelled |
| 9999 | INTERNAL_ERROR | Internal server error |

## Authentication Mechanism

HiveMind uses token-based authentication:

1. User login returns a `session_token`
2. All subsequent API requests must include in metadata:
   ```
   authorization: Bearer <session_token>
   ```
3. Token validity: 24 hours
4. Re-login required after token expiration

## Usage Examples

### Python Client Examples

#### User Service Example

```python
import grpc
from node_pool import nodepool_pb2_grpc, nodepool_pb2

# Create connection
channel = grpc.insecure_channel('localhost:50051')
user_stub = nodepool_pb2_grpc.UserServiceStub(channel)

# User registration
register_request = nodepool_pb2.RegisterRequest(
    username="testuser",
    password="password123"
)
register_response = user_stub.Register(register_request)
print(f"Registration result: {register_response.success}, Message: {register_response.message}")

# User login
login_request = nodepool_pb2.LoginRequest(
    username="testuser",
    password="password123"
)
login_response = user_stub.Login(login_request)
if login_response.success:
    token = login_response.token
    print(f"Login successful, Token: {token}")
    
    # Query balance
    balance_request = nodepool_pb2.GetBalanceRequest(
        username="testuser",
        token=token
    )
    balance_response = user_stub.GetBalance(balance_request)
    print(f"User balance: {balance_response.balance}")
```

#### Node Management Service Example

```python
# Node management service
node_stub = nodepool_pb2_grpc.NodeManagerServiceStub(channel)

# Register worker node
register_node_request = nodepool_pb2.RegisterWorkerNodeRequest(
    node_id="worker-001",
    hostname="192.168.1.100",
    cpu_cores=8,
    memory_gb=16,
    cpu_score=1000,
    gpu_score=2000,
    gpu_memory_gb=8,
    location="Asia/Taipei",
    port=50052,
    gpu_name="RTX 4090",
    trust_level=100,
    last_heartbeat=1640995200.0,
    docker_status="enabled"
)
node_response = node_stub.RegisterWorkerNode(register_node_request)
print(f"Node registration: {node_response.success}")

# Health check
health_request = nodepool_pb2.HealthCheckRequest(node_id="worker-001")
health_response = node_stub.HealthCheck(health_request)
print(f"Node health status: {health_response.status}")
```

#### Master Node Service Example

```python
# Master node service
master_stub = nodepool_pb2_grpc.MasterNodeServiceStub(channel)

# Upload task
task_data = b"print('Hello, HiveMind!')"
upload_request = nodepool_pb2.UploadTaskRequest(
    task_id="task-001",
    task_data=task_data,
    user_id="user123"
)
upload_response = master_stub.UploadTask(upload_request)
print(f"Task upload: {upload_response.success}")

# Query task status
status_request = nodepool_pb2.PollTaskStatusRequest(task_id="task-001")
status_response = master_stub.PollTaskStatus(status_request)
print(f"Task status: {status_response.status}")
print(f"Task output: {status_response.output}")
```

#### Worker Node Service Example

```python
# Worker node service  
worker_stub = nodepool_pb2_grpc.WorkerNodeServiceStub(channel)

# Report running status
status_request = nodepool_pb2.RunningStatusRequest(
    node_id="worker-001",
    task_id="task-001",
    cpu_usage=75,
    memory_usage=60,
    gpu_usage=85,
    gpu_memory_usage=70
)
status_response = worker_stub.ReportRunningStatus(status_request)
print(f"CPT reward: {status_response.cpt_reward}")
```

## Performance Considerations

- Recommend using connection pooling for gRPC connections
- Consider batch APIs for high-frequency status updates
- Use streaming APIs for large task data transfers
- Enable TLS encryption in production environments

## Limitations and Notes

1. Single task data size limit: 100MB
2. Concurrent task count limited by node resources
3. Session tokens don't support cross-node sharing
4. WebSocket real-time communication feature under development

---

**Updated**: January 2024  
**Version**: v1.0  
**Status**: Accurate documentation based on actual implementation

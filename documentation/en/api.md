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

### NodePool Service

**File**: `nodepool.proto`

```protobuf
service NodePoolService {
  // Node Management
  rpc RegisterNode(RegisterNodeRequest) returns (RegisterNodeResponse);
  rpc UnregisterNode(UnregisterNodeRequest) returns (UnregisterNodeResponse);
  rpc UpdateNodeStatus(UpdateNodeStatusRequest) returns (UpdateNodeStatusResponse);
  rpc GetNodeInfo(GetNodeInfoRequest) returns (GetNodeInfoResponse);
  
  // User Management
  rpc RegisterUser(RegisterUserRequest) returns (RegisterUserResponse);
  rpc LoginUser(LoginUserRequest) returns (LoginUserResponse);
  rpc LogoutUser(LogoutUserRequest) returns (LogoutUserResponse);
  rpc GetUserInfo(GetUserInfoRequest) returns (GetUserInfoResponse);
  
  // Task Management
  rpc SubmitTask(SubmitTaskRequest) returns (SubmitTaskResponse);
  rpc GetTaskStatus(GetTaskStatusRequest) returns (GetTaskStatusResponse);
  rpc CancelTask(CancelTaskRequest) returns (CancelTaskResponse);
  rpc GetTaskResult(GetTaskResultRequest) returns (GetTaskResultResponse);
}
```

## User Management API

### User Registration

**Method**: `RegisterUser`

**Request Parameters**:
```protobuf
message RegisterUserRequest {
  string username = 1;      // Username (3-20 chars, alphanumeric + underscore)
  string password = 2;      // Password (minimum 8 chars)
  string email = 3;         // Email address
}
```

**Response Parameters**:
```protobuf
message RegisterUserResponse {
  bool success = 1;         // Success flag
  string message = 2;       // Response message
  string user_id = 3;       // User ID (returned on success)
}
```

**Error Codes**:
- `USER_EXISTS`: Username already exists
- `INVALID_EMAIL`: Invalid email format
- `WEAK_PASSWORD`: Password strength insufficient

### User Login

**Method**: `LoginUser`

**Request Parameters**:
```protobuf
message LoginUserRequest {
  string username = 1;      // Username
  string password = 2;      // Password
}
```

**Response Parameters**:
```protobuf
message LoginUserResponse {
  bool success = 1;         // Success flag
  string message = 2;       // Response message
  string session_token = 3; // Session token
  UserInfo user_info = 4;   // User information
}
```

## Node Management API

### Node Registration

**Method**: `RegisterNode`

**Request Parameters**:
```protobuf
message RegisterNodeRequest {
  string node_id = 1;       // Node ID
  string node_type = 2;     // Node type (worker/master)
  string hostname = 3;      // Hostname
  int32 port = 4;          // Port number
  NodeCapabilities capabilities = 5; // Node capabilities
}
```

**Response Parameters**:
```protobuf
message RegisterNodeResponse {
  bool success = 1;         // Success flag
  string message = 2;       // Response message
  string assigned_id = 3;   // Assigned node ID
}
```

### Node Status Update

**Method**: `UpdateNodeStatus`

**Request Parameters**:
```protobuf
message UpdateNodeStatusRequest {
  string node_id = 1;       // Node ID
  NodeStatus status = 2;    // Node status
  ResourceUsage resource_usage = 3; // Resource usage
}
```

## Task Management API

### Submit Task

**Method**: `SubmitTask`

**Request Parameters**:
```protobuf
message SubmitTaskRequest {
  string user_id = 1;       // User ID
  string task_type = 2;     // Task type
  bytes task_data = 3;      // Task data
  TaskRequirements requirements = 4; // Task requirements
  TaskPriority priority = 5; // Task priority
}
```

**Response Parameters**:
```protobuf
message SubmitTaskResponse {
  bool success = 1;         // Success flag
  string message = 2;       // Response message
  string task_id = 3;       // Task ID
  string estimated_time = 4; // Estimated completion time
}
```

### Query Task Status

**Method**: `GetTaskStatus`

**Request Parameters**:
```protobuf
message GetTaskStatusRequest {
  string task_id = 1;       // Task ID
  string user_id = 2;       // User ID
}
```

**Response Parameters**:
```protobuf
message GetTaskStatusResponse {
  bool success = 1;         // Success flag
  string message = 2;       // Response message
  TaskStatus status = 3;    // Task status
  float progress = 4;       // Completion progress (0.0-1.0)
  string assigned_node = 5; // Assigned node
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

### Python Client Example

```python
import grpc
from node_pool import nodepool_pb2_grpc, nodepool_pb2

# Create connection
channel = grpc.insecure_channel('localhost:50051')
stub = nodepool_pb2_grpc.NodePoolServiceStub(channel)

# User registration
request = nodepool_pb2.RegisterUserRequest(
    username="testuser",
    password="password123",
    email="test@example.com"
)
response = stub.RegisterUser(request)
print(f"Registration result: {response.success}, Message: {response.message}")

# User login
login_request = nodepool_pb2.LoginUserRequest(
    username="testuser",
    password="password123"
)
login_response = stub.LoginUser(login_request)
if login_response.success:
    session_token = login_response.session_token
    print(f"Login successful, Token: {session_token}")
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

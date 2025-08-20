# TaskWorker Module Documentation

## Overview

TaskWorker is a distributed task execution library similar to Cloudflare Worker, designed to allow users to execute custom computational tasks on the HiveMind network.

## Design Philosophy

TaskWorker is designed to replace traditional Docker container execution environments, providing:
- More secure execution environment (removing unsafe system dependencies)
- Distributed file storage
- Remote key management
- DNS proxy resolution
- Remote function calls (RPC)

## Core Features

### 1. Distributed File Storage

Store files in fragments across multiple Worker nodes, providing:
- Automatic file sharding and reassembly
- Version control and synchronization
- Fault tolerance and recovery mechanisms

```python
# File upload example
response = await storage.Push(PushRequest(
    file_data=file_content,
    filename="example.txt",
    user_id="user123"
))
```

### 2. Remote Function Calls

Wrap Python functions as RPC services, allowing node pool proxy calls:

```python
from taskworker import TaskWorker

worker = TaskWorker("worker_001")

@worker.function("calculate")
def calculate_result(x, y):
    return x + y

@worker.function("process_data")  
def process_data(data):
    # Data processing logic
    return {"processed": True, "result": data}
```

### 3. Secure Execution Environment

- Remove dangerous system calls (such as `os` module)
- Restrict network access, only allow proxy through node pool
- Sandboxed execution environment

### 4. Key Management

Securely obtain and manage keys through the node pool:

```python
# Get key from node pool
api_key = worker.get_secret("external_api_key")

# Use key for external API calls
result = worker.call_external_api("https://api.example.com", 
                                  headers={"Authorization": f"Bearer {api_key}"})
```

## API Interface

### gRPC Service Definitions

TaskWorker provides three main gRPC services:

#### 1. FileService - File Operation Service

```protobuf
service FileService {
    rpc Push(PushRequest) returns (PushResponse);           // Upload file
    rpc Get(GetRequest) returns (GetResponse);              // Get file
    rpc Revise(ReviseRequest) returns (ReviseResponse);     // Revise file
    rpc Synchronous(SynchronousRequest) returns (SynchronousResponse); // Sync file
}
```

#### 2. RPCService - Remote Function Call Service

```protobuf
service RPCService {
    rpc CallFunction(FunctionCallRequest) returns (FunctionCallResponse);
}
```

#### 3. DNSService - DNS Proxy Service

```protobuf
service DNSService {
    rpc ResolveDomain(DNSRequest) returns (DNSResponse);
}
```

## Practical Usage Examples

### Basic Setup

```python
import asyncio
from taskworker import TaskWorker

# Create TaskWorker instance
worker = TaskWorker("worker_001")

# Register functions
@worker.function("hello")
def hello_world():
    return "Hello from HiveMind!"

@worker.function("add")
def add_numbers(a, b):
    return a + b

async def main():
    # Start server
    await worker.start_server(port=50052)

if __name__ == "__main__":
    asyncio.run(main())
```

### File Operation Examples

```python
# Upload file
async def upload_file():
    with open("data.txt", "rb") as f:
        content = f.read()
    
    response = await worker.storage.Push(
        taskworker_pb2.PushRequest(
            file_data=content,
            filename="data.txt",
            user_id="user123"
        )
    )
    return response.file_id

# Download file
async def download_file(file_id):
    response = await worker.storage.Get(
        taskworker_pb2.GetRequest(
            file_id=file_id,
            user_id="user123"
        )
    )
    return response.file_data
```

## Architecture Integration

### Interaction with Node Pool

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   User Request  │    │   Node Pool     │    │   TaskWorker    │
│                 │    │   Proxy         │    │                 │
│ DNS Resolution  │───►│ Route to        │───►│ Execute         │
│ Function Calls  │    │ Corresponding   │    │ Function Calls  │
│ File Operations │◄───│ Worker Node     │◄───│ Return Results  │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Lifecycle Management

1. **Initialization**: TaskWorker starts and registers with node pool
2. **Function Registration**: Register user-defined functions as RPC services
3. **Request Processing**: Receive and process proxy requests from node pool
4. **Status Monitoring**: Master node monitors TaskWorker performance and availability
5. **Fault Tolerance**: When Worker goes offline, tasks automatically migrate to other nodes

## Development Status

### ✅ Implemented Features

- Basic TaskWorker framework
- gRPC service definitions and implementations
- Function registration and call mechanism
- Distributed file storage infrastructure

### 🚧 In Development

- Complete file sharding and synchronization mechanism
- Key management system integration with node pool
- DNS proxy functionality implementation
- Secure sandbox execution environment

### 📋 Planned Features

- Automatic load balancing
- Task migration and recovery
- More security restrictions and monitoring
- Performance optimization and caching mechanisms

## Technical Implementation

### Core Classes

```python
class TaskWorker:
    """Main TaskWorker class"""
    def __init__(self, worker_id: str, node_pool_address: str)
    def register_function(self, name: str, func: Callable)
    def function(self, name: str = None)  # Decorator
    async def start_server(self, port: int = 50052)
    async def stop_server(self)

class FileStorage:
    """Distributed file storage manager"""
    async def Push(self, request, context)
    async def Get(self, request, context)
    async def Revise(self, request, context)
    async def Synchronous(self, request, context)

class RPCService:
    """RPC function call service"""
    async def CallFunction(self, request, context)
```

## Important Notes

- This is a library, not a complete system, and needs to work with existing HiveMind infrastructure
- Removed unsafe system modules like `os` to ensure execution environment security
- All external network requests need to go through node pool proxy
- File storage uses sharding mechanism to ensure data security and availability

## Future Plans

1. **Replace Docker**: Eventually replace existing Docker container execution environment
2. **Enhanced Security**: Further restrict available Python modules and functionality
3. **Performance Optimization**: Implement more efficient task scheduling and resource management
4. **Ecosystem Building**: Build rich task templates and best practices library

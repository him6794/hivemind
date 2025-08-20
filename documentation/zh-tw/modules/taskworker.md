# TaskWorker 模組文檔

## 概述

TaskWorker 是一個類似於 Cloudflare Worker 的分散式任務執行庫，讓使用者能夠在 HiveMind 網路上執行自定義的運算任務。

## 設計理念

TaskWorker 設計用於替代傳統的 Docker 容器執行環境，提供：
- 更安全的執行環境（移除不安全的系統依賴）
- 分散式文件存儲
- 遠端密鑰管理
- DNS 代理解析
- 函數遠端調用 (RPC)

## 核心功能

### 1. 分散式文件存儲

將文件分片存儲在多個 Worker 節點上，提供：
- 自動文件分片和重組
- 版本控制和同步
- 容錯和恢復機制

```python
# 文件上傳示例
response = await storage.Push(PushRequest(
    file_data=file_content,
    filename="example.txt",
    user_id="user123"
))
```

### 2. 函數遠端調用

將 Python 函數包裝成 RPC 服務，允許節點池代理調用：

```python
from taskworker import TaskWorker

worker = TaskWorker("worker_001")

@worker.function("calculate")
def calculate_result(x, y):
    return x + y

@worker.function("process_data")  
def process_data(data):
    # 處理數據邏輯
    return {"processed": True, "result": data}
```

### 3. 安全執行環境

- 移除危險的系統調用（如 `os` 模組）
- 限制網路訪問，僅允許通過節點池代理
- 沙盒化執行環境

### 4. 密鑰管理

通過節點池安全獲取和管理密鑰：

```python
# 從節點池獲取密鑰
api_key = worker.get_secret("external_api_key")

# 使用密鑰進行外部 API 調用
result = worker.call_external_api("https://api.example.com", 
                                  headers={"Authorization": f"Bearer {api_key}"})
```

## API 接口

### gRPC 服務定義

TaskWorker 提供三個主要的 gRPC 服務：

#### 1. FileService - 文件操作服務

```protobuf
service FileService {
    rpc Push(PushRequest) returns (PushResponse);           // 上傳文件
    rpc Get(GetRequest) returns (GetResponse);              // 獲取文件
    rpc Revise(ReviseRequest) returns (ReviseResponse);     // 修正文件
    rpc Synchronous(SynchronousRequest) returns (SynchronousResponse); // 同步文件
}
```

#### 2. RPCService - 遠端函數調用服務

```protobuf
service RPCService {
    rpc CallFunction(FunctionCallRequest) returns (FunctionCallResponse);
}
```

#### 3. DNSService - DNS 代理服務

```protobuf
service DNSService {
    rpc ResolveDomain(DNSRequest) returns (DNSResponse);
}
```

## 實際使用範例

### 基本設置

```python
import asyncio
from taskworker import TaskWorker

# 創建 TaskWorker 實例
worker = TaskWorker("worker_001")

# 註冊函數
@worker.function("hello")
def hello_world():
    return "Hello from HiveMind!"

@worker.function("add")
def add_numbers(a, b):
    return a + b

async def main():
    # 啟動服務器
    await worker.start_server(port=50052)

if __name__ == "__main__":
    asyncio.run(main())
```

### 文件操作範例

```python
# 上傳文件
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

# 下載文件
async def download_file(file_id):
    response = await worker.storage.Get(
        taskworker_pb2.GetRequest(
            file_id=file_id,
            user_id="user123"
        )
    )
    return response.file_data
```

## 架構集成

### 與節點池的交互

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   用戶請求       │    │   節點池代理     │    │   TaskWorker    │
│                 │    │                 │    │                 │
│ DNS 解析請求     │───►│ 路由到對應       │───►│ 執行函數調用     │
│ 函數調用請求     │    │ Worker 節點     │    │ 返回結果         │
│ 文件操作請求     │◄───│ 返回結果        │◄───│                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### 生命週期管理

1. **初始化**: TaskWorker 啟動並註冊到節點池
2. **函數註冊**: 將用戶定義的函數註冊為 RPC 服務
3. **請求處理**: 接收並處理來自節點池的代理請求
4. **狀態監控**: Master 節點監控 TaskWorker 的性能和可用性
5. **容錯處理**: 當 Worker 下線時，任務自動遷移到其他節點

## 開發狀態

### ✅ 已實現功能

- 基本的 TaskWorker 框架
- gRPC 服務定義和實現
- 函數註冊和調用機制
- 分散式文件存儲基礎結構

### 🚧 開發中功能

- 完整的文件分片和同步機制
- 密鑰管理系統與節點池集成
- DNS 代理功能實現
- 安全沙盒執行環境

### 📋 計劃功能

- 自動負載平衡
- 任務遷移和恢復
- 更多安全限制和監控
- 性能優化和快取機制

## 技術實現

### 核心類別

```python
class TaskWorker:
    """主要的 TaskWorker 類別"""
    def __init__(self, worker_id: str, node_pool_address: str)
    def register_function(self, name: str, func: Callable)
    def function(self, name: str = None)  # 裝飾器
    async def start_server(self, port: int = 50052)
    async def stop_server(self)

class FileStorage:
    """分散式文件存儲管理器"""
    async def Push(self, request, context)
    async def Get(self, request, context)
    async def Revise(self, request, context)
    async def Synchronous(self, request, context)

class RPCService:
    """RPC 函數調用服務"""
    async def CallFunction(self, request, context)
```

## 注意事項

- 這是一個庫而非完整系統，需要與現有的 HiveMind 基礎設施配合使用
- 移除了 `os` 等不安全的系統模組，確保執行環境安全
- 所有外部網路請求需要通過節點池代理
- 文件存儲採用分片機制，確保數據安全和可用性

## 後續計劃

1. **替代 Docker**: 最終將替代現有的 Docker 容器執行環境
2. **增強安全性**: 進一步限制可用的 Python 模組和功能
3. **性能優化**: 實現更高效的任務調度和資源管理
4. **生態建設**: 建立豐富的任務模板和最佳實踐庫

# HiveMind 任務日誌查看指南

## 問題說明

當你看到 "task completed" 但沒有輸出日誌時，這是因為：

1. **任務輸出存儲在內存中**: Nodepool 將任務的輸出（stdout/stderr）存儲在內存的 `taskState.Output` 字段中
2. **不會自動打印到控制台**: 任務完成後，輸出不會自動顯示在 nodepool.log 文件中
3. **需要主動查詢**: 你需要通過 `GetTasklog` RPC 調用來獲取任務的輸出日誌

## 解決方案

### 方法 1: 使用提供的工具腳本

我已經創建了 `get_task_log.py` 工具來幫助你查看任務日誌。

#### 基本用法

```bash
# 列出所有任務
python get_task_log.py --list

# 查看特定任務的日誌
python get_task_log.py --task-id <任務ID>

# 使用自定義 nodepool 地址和憑證
python get_task_log.py --nodepool localhost:50051 --user testuser --password testpass123
```

#### 示例

```bash
# 1. 列出所有任務
python get_task_log.py --list

# 輸出:
# 找到 3 個任務:
#   任務 ID: task-1777351996083520400
#   狀態: COMPLETED
#   訊息: result uploaded
#   ...

# 2. 查看特定任務的日誌
python get_task_log.py --task-id task-1777351996083520400

# 輸出:
# ============================================================
# 任務 ID: task-1777351996083520400
# ============================================================
# [任務輸出內容]
# ============================================================
```

### 方法 2: 使用 gRPC 客戶端

如果你有自己的客戶端代碼，可以這樣調用：

```python
import grpc
import hivemind_pb2
import hivemind_pb2_grpc

# 連接到 Nodepool
channel = grpc.insecure_channel('localhost:50051')
stub = hivemind_pb2_grpc.MasterNodeServiceStub(channel)

# 獲取任務日誌
response = stub.GetTasklog(
    hivemind_pb2.TasklogRequest(
        token=your_token,
        task_id=your_task_id
    )
)

if response.success:
    print(response.log)
else:
    print(f"錯誤: {response.log}")
```

### 方法 3: 通過 Master UI 查看

如果你使用 Master UI (http://localhost:3000):

1. 登入系統
2. 進入「任務列表」頁面
3. 點擊任務查看詳情
4. 在任務詳情頁面可以看到輸出日誌

## 任務日誌的工作原理

### 日誌收集流程

```
Worker 執行任務
    ↓
Worker 調用 TaskOutputUpload RPC
    ↓
Nodepool 接收輸出並存儲到 taskState.Output
    ↓
用戶調用 GetTasklog RPC
    ↓
Nodepool 返回 taskState.Output
```

### 相關代碼

#### 1. Worker 上傳輸出 (services/worker)

```go
// Worker 執行任務後上傳輸出
resp, err := client.TaskOutputUpload(ctx, &pb.TaskOutputUploadRequest{
    TaskId: taskID,
    Output: stdout,  // 任務的 stdout
})
```

#### 2. Nodepool 接收輸出 (services/nodepool/cmd/server/main.go)

```go
// TaskOutputUpload RPC handler
func (w *workerIngressServer) TaskOutputUpload(ctx context.Context, req *pb.TaskOutputUploadRequest) (*pb.TaskOutputUploadResponse, error) {
    // ...
    w.master.setTaskOutput(req.GetTaskId(), req.GetOutput())
    // ...
}

// 存儲輸出到內存
func (m *masterNodeServer) setTaskOutput(taskID, output string) bool {
    // ...
    appendTaskLogLocked(t, output)  // 追加到 t.Output
    // ...
}
```

#### 3. 用戶查詢日誌 (services/nodepool/cmd/server/main.go)

```go
// GetTasklog RPC handler
func (m *masterNodeServer) GetTasklog(ctx context.Context, req *pb.TasklogRequest) (*pb.TasklogResponse, error) {
    // ...
    t, ok := m.getTask(req.GetTaskId())
    if t.Output == "" {
        return &pb.TasklogResponse{Success: false, Log: "log not ready"}, nil
    }
    return &pb.TasklogResponse{Success: true, Log: t.Output}, nil
}
```

## 常見問題

### Q1: 為什麼任務完成了但 GetTasklog 返回 "log not ready"？

**原因**: Worker 可能還沒有上傳輸出，或者上傳失敗了。

**解決方案**:
1. 檢查 Worker 是否正常運行
2. 檢查網絡連接
3. 查看 Worker 日誌確認是否有錯誤

### Q2: 任務日誌會持久化嗎？

**答案**: 目前任務日誌存儲在內存中（`taskState.Output`），如果 Nodepool 重啟，日誌會丟失。

**建議**: 
- 在生產環境中，應該將日誌持久化到數據庫或文件系統
- 可以修改代碼將 `Output` 字段存儲到 PostgreSQL

### Q3: 日誌大小有限制嗎？

**答案**: 目前沒有明確的大小限制，但存儲在內存中可能會導致內存問題。

**建議**:
- 對於大量輸出的任務，考慮將日誌寫入文件
- 或者實現日誌分頁/截斷機制

### Q4: 如何在 nodepool.log 中看到任務輸出？

**答案**: 目前 `setTaskOutput` 函數不會將輸出寫入 nodepool.log。

**解決方案**: 如果需要，可以修改代碼添加日誌記錄：

```go
func (m *masterNodeServer) setTaskOutput(taskID, output string) bool {
    m.mu.Lock()
    defer m.mu.Unlock()
    t, ok := m.tasks[taskID]
    if !ok || t == nil {
        return false
    }
    appendTaskLogLocked(t, output)
    
    // 添加這行來記錄到 nodepool.log
    log.Printf("task_output_received task_id=%s output=%s", taskID, output)
    
    // ... 其餘代碼
}
```

## 改進建議

### 1. 添加日誌持久化

修改 `saveTaskLocked` 函數，將 `Output` 字段保存到數據庫：

```go
func (m *masterNodeServer) saveTaskLocked(t *taskState) {
    // 已有的代碼...
    _, _ = m.db.Exec(`INSERT INTO tasks(..., output, ...) VALUES(..., $7, ...)
        ON CONFLICT(task_id) DO UPDATE SET ..., output=EXCLUDED.output, ...`,
        ..., t.Output, ...)
}
```

### 2. 添加日誌文件記錄

在 `setTaskOutput` 中添加文件日誌：

```go
func (m *masterNodeServer) setTaskOutput(taskID, output string) bool {
    // ... 現有代碼
    
    // 記錄到日誌文件
    log.Printf("task_output_received task_id=%s length=%d", taskID, len(output))
    
    // 可選：記錄完整輸出（注意可能很長）
    if len(output) < 1000 {
        log.Printf("task_output task_id=%s output=%s", taskID, output)
    }
    
    // ... 其餘代碼
}
```

### 3. 實現日誌流式傳輸

對於長時間運行的任務，可以實現流式日誌傳輸：

```protobuf
service MasterNodeService {
    rpc StreamTasklog(TasklogRequest) returns (stream TasklogChunk);
}
```

## 總結

- 任務輸出存儲在 Nodepool 的內存中
- 使用 `GetTasklog` RPC 或 `get_task_log.py` 工具查看日誌
- 考慮添加日誌持久化和文件記錄功能
- Master UI 提供了友好的日誌查看界面

---

**工具腳本**: `get_task_log.py`  
**相關文件**: `services/nodepool/cmd/server/main.go`  
**更新時間**: 2026/04/28

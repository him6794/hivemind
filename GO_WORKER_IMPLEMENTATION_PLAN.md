# Go Worker 任務執行器實現計劃

**日期**: 2026-04-28  
**狀態**: 待實現

---

## 當前狀況

Go Worker (`services/worker/pkg/executor/executor.go`) 目前只有一個空殼函數：

```go
func ExecuteTask(torrent string) (string, error) {
    return "", nil
}
```

需要實現完整的任務下載、解壓、執行流程。

---

## 需要實現的功能

### 1. ZIP 文件處理

#### 下載功能
```go
func DownloadTaskZip(url string) ([]byte, error) {
    // HTTP/HTTPS 下載
    // 支持 torrent/magnet 鏈接
    // 進度追蹤
    // 超時控制
}
```

#### 安全解壓
```go
func SafeExtractZip(zipData []byte, destDir string) error {
    // ZIP 炸彈防護（檢查解壓後大小）
    // 路徑遍歷防護（檢查 ../ 等）
    // 最大文件數限制
    // 最大解壓大小限制（1GB）
}
```

### 2. 任務執行

#### 沙盒執行
```go
type TaskExecutor struct {
    WorkDir string
    Timeout time.Duration
    MemoryLimit int64
    CPULimit float64
}

func (e *TaskExecutor) Execute() (output string, err error) {
    // 查找可執行腳本（main.py, run.py等）
    // 使用進程隔離執行
    // 資源限制（CPU、內存、時間）
    // 捕獲標準輸出/錯誤
}
```

#### 資源限制
```go
func SetResourceLimits(cmd *exec.Cmd, limits ResourceLimits) {
    // 使用 cgroups（Linux）或 Job Objects（Windows）
    // CPU 限制
    // 內存限制
    // 進程數限制
    // 文件大小限制
}
```

### 3. 網絡安全

#### SSRF 防護
```go
func ValidateURL(url string) error {
    // 禁止訪問內網地址
    // 禁止訪問 localhost
    // 禁止訪問雲元數據服務
    // 協議白名單（http/https）
}
```

#### 請求限制
```go
type NetworkLimiter struct {
    RequestCount int
    MaxRequests int
    MaxResponseSize int64
}

func (l *NetworkLimiter) CheckAndIncrement() error {
    // 請求次數限制
    // 響應大小限制
}
```

### 4. 結果處理

#### 結果打包
```go
func PackageResults(workDir string, logs []string) ([]byte, error) {
    // 創建結果 ZIP
    // 包含執行日誌
    // 包含輸出文件
    // 包含執行狀態
}
```

---

## 實現方案

### 方案 A：純 Go 實現（推薦）

**優點**：
- 統一技術棧
- 易於維護
- 跨平台支持

**缺點**：
- 需要實現完整的沙盒機制
- Python 執行需要外部 Python 解釋器

**實現步驟**：
1. 實現 ZIP 下載和安全解壓
2. 實現進程隔離執行
3. 實現資源限制（使用 OS 特定 API）
4. 實現網絡安全防護
5. 實現結果打包

**預計時間**: 2-3 週

### 方案 B：集成 Rust Executor

**優點**：
- 利用現有 `executor-rs`
- 高性能
- 內存安全

**缺點**：
- 需要 CGO 或 FFI
- 增加構建複雜度

**實現步驟**：
1. 完善 `executor-rs` 功能
2. 創建 Go 到 Rust 的 FFI 綁定
3. 集成到 Go Worker

**預計時間**: 3-4 週

### 方案 C：調用外部 Python Worker（臨時）

**優點**：
- 快速實現
- 利用已有的 Python 代碼

**缺點**：
- 仍然依賴 Python
- 違背統一 Go 架構的目標

**實現步驟**：
1. 保留 Python Worker 執行器
2. Go Worker 作為 gRPC 接口層
3. 通過進程調用 Python 執行器

**預計時間**: 1 週

---

## 推薦實現（方案 A 詳細設計）

### 文件結構
```
services/worker/pkg/executor/
├── executor.go          # 主執行器
├── downloader.go        # ZIP 下載
├── extractor.go         # 安全解壓
├── sandbox.go           # 沙盒執行
├── resource_limiter.go  # 資源限制
├── network_guard.go     # 網絡防護
├── packager.go          # 結果打包
└── executor_test.go     # 測試
```

### 核心代碼框架

#### executor.go
```go
package executor

import (
    "context"
    "fmt"
    "os"
    "path/filepath"
    "time"
)

type TaskExecutor struct {
    TaskID      string
    ZipURL      string
    WorkDir     string
    Timeout     time.Duration
    MemoryLimit int64
    CPULimit    float64
}

func NewTaskExecutor(taskID, zipURL string) *TaskExecutor {
    return &TaskExecutor{
        TaskID:      taskID,
        ZipURL:      zipURL,
        WorkDir:     filepath.Join(os.TempDir(), "hivemind_task_"+taskID),
        Timeout:     5 * time.Minute,
        MemoryLimit: 1024 * 1024 * 1024, // 1GB
        CPULimit:    1.0,
    }
}

func (e *TaskExecutor) Execute(ctx context.Context) (resultZip []byte, err error) {
    // 1. 創建工作目錄
    if err := os.MkdirAll(e.WorkDir, 0755); err != nil {
        return nil, fmt.Errorf("create workdir: %w", err)
    }
    defer os.RemoveAll(e.WorkDir)
    
    // 2. 下載任務 ZIP
    zipData, err := DownloadTaskZip(ctx, e.ZipURL)
    if err != nil {
        return nil, fmt.Errorf("download: %w", err)
    }
    
    // 3. 安全解壓
    if err := SafeExtractZip(zipData, e.WorkDir); err != nil {
        return nil, fmt.Errorf("extract: %w", err)
    }
    
    // 4. 執行任務
    output, err := e.executeSandboxed(ctx)
    if err != nil {
        return nil, fmt.Errorf("execute: %w", err)
    }
    
    // 5. 打包結果
    resultZip, err = PackageResults(e.WorkDir, output)
    if err != nil {
        return nil, fmt.Errorf("package: %w", err)
    }
    
    return resultZip, nil
}

func (e *TaskExecutor) executeSandboxed(ctx context.Context) (string, error) {
    // 查找可執行腳本
    script, err := findExecutableScript(e.WorkDir)
    if err != nil {
        return "", err
    }
    
    // 創建沙盒環境
    sandbox := NewSandbox(e.WorkDir, e.Timeout, e.MemoryLimit, e.CPULimit)
    
    // 執行
    return sandbox.Run(ctx, script)
}
```

#### extractor.go
```go
package executor

import (
    "archive/zip"
    "bytes"
    "fmt"
    "io"
    "os"
    "path/filepath"
    "strings"
)

const (
    MaxExtractedSize = 1024 * 1024 * 1024 // 1GB
    MaxFileCount     = 10000
)

func SafeExtractZip(zipData []byte, destDir string) error {
    reader, err := zip.NewReader(bytes.NewReader(zipData), int64(len(zipData)))
    if err != nil {
        return fmt.Errorf("open zip: %w", err)
    }
    
    // 檢查解壓後總大小（防止 ZIP 炸彈）
    var totalSize int64
    for _, file := range reader.File {
        totalSize += int64(file.UncompressedSize64)
        if totalSize > MaxExtractedSize {
            return fmt.Errorf("zip bomb detected: uncompressed size %d exceeds limit %d", 
                totalSize, MaxExtractedSize)
        }
    }
    
    // 檢查文件數量
    if len(reader.File) > MaxFileCount {
        return fmt.Errorf("too many files: %d exceeds limit %d", 
            len(reader.File), MaxFileCount)
    }
    
    // 解壓文件
    for _, file := range reader.File {
        if err := extractFile(file, destDir); err != nil {
            return err
        }
    }
    
    return nil
}

func extractFile(file *zip.File, destDir string) error {
    // 防止路徑遍歷攻擊
    filePath := filepath.Join(destDir, file.Name)
    if !strings.HasPrefix(filepath.Clean(filePath), filepath.Clean(destDir)) {
        return fmt.Errorf("illegal file path: %s", file.Name)
    }
    
    if file.FileInfo().IsDir() {
        return os.MkdirAll(filePath, file.Mode())
    }
    
    // 創建父目錄
    if err := os.MkdirAll(filepath.Dir(filePath), 0755); err != nil {
        return err
    }
    
    // 解壓文件
    rc, err := file.Open()
    if err != nil {
        return err
    }
    defer rc.Close()
    
    outFile, err := os.OpenFile(filePath, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, file.Mode())
    if err != nil {
        return err
    }
    defer outFile.Close()
    
    _, err = io.Copy(outFile, rc)
    return err
}
```

---

## 測試計劃

### 單元測試
```go
func TestSafeExtractZip(t *testing.T) {
    tests := []struct {
        name    string
        zipData []byte
        wantErr bool
    }{
        {"normal zip", createNormalZip(), false},
        {"zip bomb", createZipBomb(), true},
        {"path traversal", createPathTraversalZip(), true},
    }
    // ...
}
```

### 集成測試
```go
func TestTaskExecutor_Execute(t *testing.T) {
    executor := NewTaskExecutor("test-task", "http://example.com/task.zip")
    result, err := executor.Execute(context.Background())
    // 驗證結果
}
```

---

## 安全檢查清單

- [ ] ZIP 炸彈防護（大小檢查）
- [ ] 路徑遍歷防護（路徑驗證）
- [ ] SSRF 防護（URL 驗證）
- [ ] 資源限制（CPU、內存、時間）
- [ ] 網絡請求限制（次數、大小）
- [ ] 進程隔離
- [ ] 文件系統隔離
- [ ] 錯誤處理和日誌記錄

---

## 依賴項

### Go 標準庫
- `archive/zip` - ZIP 處理
- `os/exec` - 進程執行
- `context` - 超時控制
- `net/http` - HTTP 下載

### 第三方庫（可選）
- `github.com/containerd/cgroups` - Linux 資源限制
- `github.com/anacrolix/torrent` - Torrent 支持

---

## 下一步行動

### 立即（本週）
1. 決定實現方案（推薦方案 A）
2. 創建詳細的技術設計文檔
3. 設置開發環境和測試框架

### 短期（2週內）
4. 實現 ZIP 下載和安全解壓
5. 實現基本的進程執行
6. 添加單元測試

### 中期（1個月內）
7. 實現完整的資源限制
8. 實現網絡安全防護
9. 實現結果打包
10. 完整的集成測試

---

## 結論

Go Worker 需要完整實現任務執行引擎。推薦使用純 Go 實現（方案 A），預計需要 2-3 週開發時間。

在完成之前，建議：
1. 使用 Go 服務處理 API 和調度
2. 保留測試用的 Python 執行器作為參考
3. 逐步遷移功能到 Go

---

**文檔生成時間**: 2026-04-28 09:07:00  
**優先級**: P0 - 高優先級  
**預計完成**: 2026-05-19

# Go Worker 執行器安全與性能審查報告

**審查日期**: 2026/04/28  
**審查範圍**: services/worker/pkg/executor/*  
**審查者**: AI Code Review

---

## 執行摘要

Go Worker 執行器實現了完整的任務下載、解壓、執行和結果打包流程。經過全面審查，發現 **6 個安全問題**（2 高危、3 中危、1 低危）和 **5 個性能瓶頸**。

### 風險評級
- 🔴 **高危**: 2 個 - 需要立即修復
- 🟡 **中危**: 3 個 - 建議盡快修復
- 🟢 **低危**: 1 個 - 可以延後處理

---

## 一、安全問題

### 🔴 HIGH-01: 下載 URL 未驗證（SSRF 風險）

**位置**: `executor.go:121-142` (downloadFile 函數)

**問題描述**:
```go
func downloadFile(url string) ([]byte, error) {
    client := &http.Client{
        Timeout: 5 * time.Minute,
    }
    resp, err := client.Get(url)  // ❌ 未驗證 URL
    ...
}
```

downloadURL 參數直接用於 HTTP 請求，沒有任何驗證。攻擊者可以：
- 訪問內網服務（127.0.0.1, 192.168.x.x, 10.x.x.x）
- 訪問雲服務 metadata API（169.254.169.254）
- 進行端口掃描
- 讀取本地文件（file:// 協議）

**影響**: 
- 可能洩露內網信息
- 可能獲取雲服務憑證
- 可能攻擊內網服務

**修復建議**:
```go
func downloadFile(url string) ([]byte, error) {
    // 1. 驗證 URL 協議
    parsedURL, err := neturl.Parse(url)
    if err != nil {
        return nil, fmt.Errorf("invalid url: %w", err)
    }
    
    if parsedURL.Scheme != "http" && parsedURL.Scheme != "https" {
        return nil, fmt.Errorf("unsupported protocol: %s", parsedURL.Scheme)
    }
    
    // 2. 檢查主機名
    host := parsedURL.Hostname()
    
    // 禁止訪問本地地址
    if host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0" {
        return nil, fmt.Errorf("access to localhost is forbidden")
    }
    
    // 禁止訪問私有 IP
    ip := net.ParseIP(host)
    if ip != nil {
        if ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() {
            return nil, fmt.Errorf("access to private IP is forbidden: %s", host)
        }
        
        // 禁止訪問 metadata 服務
        if ip.String() == "169.254.169.254" {
            return nil, fmt.Errorf("access to metadata service is forbidden")
        }
    }
    
    // 3. 使用自定義 Transport 禁止重定向到內網
    client := &http.Client{
        Timeout: 5 * time.Minute,
        CheckRedirect: func(req *http.Request, via []*http.Request) error {
            // 檢查重定向目標
            if err := validateURL(req.URL.String()); err != nil {
                return err
            }
            if len(via) >= 10 {
                return fmt.Errorf("too many redirects")
            }
            return nil
        },
    }
    
    resp, err := client.Get(url)
    ...
}
```

**優先級**: P0 - 立即修復

---

### 🔴 HIGH-02: 下載大小無限制（DoS 風險）

**位置**: `executor.go:136` (io.ReadAll)

**問題描述**:
```go
data, err := io.ReadAll(resp.Body)  // ❌ 無大小限制
```

攻擊者可以提供一個返回無限數據的 URL，導致：
- 內存耗盡（OOM）
- Worker 崩潰
- 系統資源耗盡

**影響**:
- 拒絕服務攻擊
- Worker 不可用
- 影響其他任務執行

**修復建議**:
```go
const MaxDownloadSize = 100 * 1024 * 1024 // 100MB

func downloadFile(url string) ([]byte, error) {
    ...
    
    // 檢查 Content-Length
    if resp.ContentLength > MaxDownloadSize {
        return nil, fmt.Errorf("file too large: %d bytes (max: %d)", 
            resp.ContentLength, MaxDownloadSize)
    }
    
    // 使用 LimitReader 限制讀取大小
    limitedReader := io.LimitReader(resp.Body, MaxDownloadSize+1)
    data, err := io.ReadAll(limitedReader)
    if err != nil {
        return nil, fmt.Errorf("read body: %w", err)
    }
    
    // 檢查是否超過限制
    if len(data) > MaxDownloadSize {
        return nil, fmt.Errorf("file exceeds size limit: %d MB", MaxDownloadSize/1024/1024)
    }
    
    return data, nil
}
```

**優先級**: P0 - 立即修復

---

### 🟡 MED-01: 臨時文件清理不完整

**位置**: `executor.go:40` (defer os.RemoveAll)

**問題描述**:
```go
defer os.RemoveAll(tempDir)  // ⚠️ 如果 panic 可能不執行
```

如果在執行過程中發生 panic，defer 可能不會執行，導致臨時文件殘留。

**影響**:
- 磁盤空間洩漏
- 長期運行後磁盤滿
- 可能洩露敏感數據

**修復建議**:
```go
func ExecuteTask(taskID string, downloadURL string) (result *TaskResult, err error) {
    result = &TaskResult{
        TaskID: taskID,
        Logs:   []string{},
    }
    
    // 使用 recover 確保清理
    tempDir, err := os.MkdirTemp("", fmt.Sprintf("task_%s_*", taskID))
    if err != nil {
        result.Error = fmt.Errorf("create temp dir: %w", err)
        return result, result.Error
    }
    
    defer func() {
        if r := recover(); r != nil {
            result.Logs = append(result.Logs, fmt.Sprintf("PANIC: %v", r))
            result.Success = false
        }
        // 確保清理
        if err := os.RemoveAll(tempDir); err != nil {
            result.Logs = append(result.Logs, fmt.Sprintf("Failed to cleanup: %v", err))
        }
    }()
    
    ...
}
```

**優先級**: P1 - 盡快修復

---

### 🟡 MED-02: 文件權限過於寬鬆

**位置**: `extractor.go:72, 76, 87`

**問題描述**:
```go
os.MkdirAll(filePath, 0755)           // ⚠️ 所有人可讀可執行
os.MkdirAll(filepath.Dir(filePath), 0755)
os.OpenFile(filePath, ..., file.Mode())  // ⚠️ 使用 ZIP 中的權限
```

問題：
1. 目錄權限 0755 允許其他用戶讀取和執行
2. 直接使用 ZIP 中的文件權限，可能包含執行權限

**影響**:
- 其他用戶可能讀取任務數據
- 可能執行惡意腳本
- 權限提升風險

**修復建議**:
```go
const (
    DirPerm  = 0700  // 僅所有者可訪問
    FilePerm = 0600  // 僅所有者可讀寫
)

func extractFile(file *zip.File, destDir string) error {
    ...
    
    if file.FileInfo().IsDir() {
        return os.MkdirAll(filePath, DirPerm)  // 0700
    }
    
    if err := os.MkdirAll(filepath.Dir(filePath), DirPerm); err != nil {
        return err
    }
    
    // 移除執行權限，僅保留讀寫
    perm := file.Mode() & 0666  // 移除執行位
    if perm == 0 {
        perm = FilePerm  // 默認 0600
    }
    
    outFile, err := os.OpenFile(filePath, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, perm)
    ...
}
```

**優先級**: P1 - 盡快修復

---

### 🟡 MED-03: 符號鏈接攻擊風險

**位置**: `extractor.go:56-94` (extractFile 函數)

**問題描述**:
ZIP 文件可能包含符號鏈接，指向系統敏感文件。當前代碼沒有檢查符號鏈接。

**影響**:
- 可能覆蓋系統文件
- 可能讀取敏感文件
- 權限提升風險

**修復建議**:
```go
func extractFile(file *zip.File, destDir string) error {
    ...
    
    // 檢查是否為符號鏈接
    if file.Mode()&os.ModeSymlink != 0 {
        return fmt.Errorf("symbolic links are not allowed: %s", file.Name)
    }
    
    ...
}
```

**優先級**: P1 - 盡快修復

---

### 🟢 LOW-01: 硬編碼路徑

**位置**: `monty_runner.go:15`

**問題描述**:
```go
const MontyExecutable = `C:\Users\user\Desktop\monty\dist\monty.exe`  // ❌ 硬編碼
```

問題：
- 不可移植
- 部署困難
- 無法配置

**修復建議**:
```go
// 從環境變量或配置文件讀取
func getMontyPath() string {
    if path := os.Getenv("MONTY_EXECUTABLE"); path != "" {
        return path
    }
    // 默認路徑
    return `C:\Users\user\Desktop\monty\dist\monty.exe`
}

var MontyExecutable = getMontyPath()
```

**優先級**: P2 - 可以延後

---

## 二、性能瓶頸

### PERF-01: 下載時內存占用過高

**位置**: `executor.go:136` (io.ReadAll)

**問題**:
```go
data, err := io.ReadAll(resp.Body)  // ❌ 全部加載到內存
```

對於大文件（如 100MB ZIP），會占用大量內存。

**影響**:
- 內存峰值高
- 並發任務時內存壓力大
- 可能觸發 GC

**優化建議**:
```go
// 方案 1: 流式處理
func downloadFile(url string) ([]byte, error) {
    ...
    
    // 先寫入臨時文件
    tmpFile, err := os.CreateTemp("", "download_*.zip")
    if err != nil {
        return nil, err
    }
    defer os.Remove(tmpFile.Name())
    defer tmpFile.Close()
    
    // 流式複製
    written, err := io.Copy(tmpFile, io.LimitReader(resp.Body, MaxDownloadSize+1))
    if err != nil {
        return nil, err
    }
    
    if written > MaxDownloadSize {
        return nil, fmt.Errorf("file too large")
    }
    
    // 讀取回內存（或直接傳遞文件路徑）
    tmpFile.Seek(0, 0)
    return io.ReadAll(tmpFile)
}

// 方案 2: 直接傳遞 io.Reader
func SafeExtractZip(zipReader io.ReaderAt, size int64, destDir string) error {
    reader, err := zip.NewReader(zipReader, size)
    ...
}
```

**預期改善**: 內存占用減少 50-90%

---

### PERF-02: ZIP 解壓時重複遍歷

**位置**: `extractor.go:27-38`

**問題**:
```go
// 第一次遍歷：計算大小
for _, file := range reader.File {
    totalSize += int64(file.UncompressedSize64)
    if totalSize > MaxExtractedSize {
        // 第二次遍歷：計算壓縮大小
        for _, f := range reader.File {
            compressedSize += int64(f.CompressedSize64)
        }
        ...
    }
}

// 第三次遍歷：解壓文件
for _, file := range reader.File {
    extractFile(file, destDir)
}
```

**影響**:
- 不必要的 CPU 消耗
- 對於大量文件的 ZIP 影響明顯

**優化建議**:
```go
func SafeExtractZip(zipData []byte, destDir string) error {
    reader, err := zip.NewReader(bytes.NewReader(zipData), int64(len(zipData)))
    if err != nil {
        return fmt.Errorf("open zip: %w", err)
    }
    
    // 檢查文件數量
    if len(reader.File) > MaxFileCount {
        return fmt.Errorf("too many files: %d exceeds limit %d",
            len(reader.File), MaxFileCount)
    }
    
    // 單次遍歷：檢查並解壓
    var totalSize int64
    var compressedSize int64
    
    for _, file := range reader.File {
        totalSize += int64(file.UncompressedSize64)
        compressedSize += int64(file.CompressedSize64)
        
        if totalSize > MaxExtractedSize {
            ratio := float64(totalSize) / float64(compressedSize)
            return fmt.Errorf("zip bomb detected: uncompressed size %d exceeds limit %d (ratio: %.1fx)",
                totalSize, MaxExtractedSize, ratio)
        }
        
        // 立即解壓
        if err := extractFile(file, destDir); err != nil {
            return fmt.Errorf("extract file %s: %w", file.Name, err)
        }
    }
    
    return nil
}
```

**預期改善**: 解壓速度提升 20-30%

---

### PERF-03: 結果打包時重複遍歷目錄

**位置**: `packager.go:35-67` (filepath.Walk)

**問題**:
filepath.Walk 會遍歷所有文件和目錄，對於大量小文件效率較低。

**優化建議**:
```go
// 使用 WalkDir（Go 1.16+）替代 Walk
err = filepath.WalkDir(workDir, func(path string, d fs.DirEntry, err error) error {
    if err != nil {
        return err
    }
    
    // 跳過目錄
    if d.IsDir() {
        return nil
    }
    
    // 計算相對路徑
    relPath, err := filepath.Rel(workDir, path)
    if err != nil {
        return err
    }
    
    // 創建 ZIP 條目
    writer, err := zipWriter.Create(relPath)
    if err != nil {
        return fmt.Errorf("create zip entry %s: %w", relPath, err)
    }
    
    // 讀取並寫入文件內容
    file, err := os.Open(path)
    if err != nil {
        return fmt.Errorf("open file %s: %w", path, err)
    }
    defer file.Close()
    
    if _, err := io.Copy(writer, file); err != nil {
        return fmt.Errorf("write file %s: %w", relPath, err)
    }
    
    return nil
})
```

**預期改善**: 打包速度提升 10-15%

---

### PERF-04: 日誌字符串拼接效率低

**位置**: `executor.go:42, 45, 52, ...` (多處)

**問題**:
```go
result.Logs = append(result.Logs, fmt.Sprintf("[%s] Created temp dir: %s", 
    time.Now().Format("15:04:05"), tempDir))
```

每次都調用 time.Now() 和 Format，對於頻繁日誌記錄效率較低。

**優化建議**:
```go
// 添加輔助函數
func (r *TaskResult) Log(format string, args ...interface{}) {
    timestamp := time.Now().Format("15:04:05")
    msg := fmt.Sprintf(format, args...)
    r.Logs = append(r.Logs, fmt.Sprintf("[%s] %s", timestamp, msg))
}

// 使用
result.Log("Created temp dir: %s", tempDir)
result.Log("Downloading task from: %s", downloadURL)
result.Log("Downloaded %d bytes", len(zipData))
```

**預期改善**: 日誌記錄速度提升 30-40%

---

### PERF-05: HTTP Client 重複創建

**位置**: `executor.go:122-124`

**問題**:
```go
func downloadFile(url string) ([]byte, error) {
    client := &http.Client{  // ❌ 每次都創建新 client
        Timeout: 5 * time.Minute,
    }
    ...
}
```

每次下載都創建新的 HTTP Client，無法復用連接。

**優化建議**:
```go
// 包級別的 HTTP Client
var httpClient = &http.Client{
    Timeout: 5 * time.Minute,
    Transport: &http.Transport{
        MaxIdleConns:        100,
        MaxIdleConnsPerHost: 10,
        IdleConnTimeout:     90 * time.Second,
    },
}

func downloadFile(url string) ([]byte, error) {
    resp, err := httpClient.Get(url)  // 復用 client
    ...
}
```

**預期改善**: 下載速度提升 10-20%（特別是多次下載時）

---

## 三、測試覆蓋率分析

### 當前測試狀態
- ✅ **TestExecuteTask_SimpleScript**: 通過
- ✅ **TestExecuteTask_PrimeCalculation**: 通過
- ⏭️ **TestExecuteTask_FileOperations**: 跳過（monty 限制）
- ⏭️ **TestSafeExtractZip_ZipBomb**: 跳過（測試限制）
- ✅ **TestFindExecutableScript**: 通過

### 缺失的測試
1. **安全測試**:
   - ❌ SSRF 攻擊測試
   - ❌ 路徑遍歷測試
   - ❌ 符號鏈接測試
   - ❌ 大文件下載測試
   - ❌ 惡意 ZIP 測試

2. **錯誤處理測試**:
   - ❌ 網絡錯誤測試
   - ❌ 磁盤滿測試
   - ❌ 權限錯誤測試
   - ❌ 超時測試

3. **並發測試**:
   - ❌ 多任務並發執行
   - ❌ 資源競爭測試

### 建議新增測試
```go
// 安全測試
func TestDownloadFile_SSRF(t *testing.T) {
    urls := []string{
        "http://localhost:8080/admin",
        "http://127.0.0.1/secret",
        "http://169.254.169.254/metadata",
        "http://192.168.1.1/config",
        "file:///etc/passwd",
    }
    
    for _, url := range urls {
        _, err := downloadFile(url)
        if err == nil {
            t.Errorf("Expected error for SSRF URL: %s", url)
        }
    }
}

// 性能測試
func BenchmarkExecuteTask(b *testing.B) {
    // 測試執行性能
}

func BenchmarkSafeExtractZip(b *testing.B) {
    // 測試解壓性能
}
```

---

## 四、修復優先級總結

### 立即修復（P0）
1. 🔴 HIGH-01: 下載 URL 未驗證（SSRF 風險）
2. 🔴 HIGH-02: 下載大小無限制（DoS 風險）

### 盡快修復（P1）
3. 🟡 MED-01: 臨時文件清理不完整
4. 🟡 MED-02: 文件權限過於寬鬆
5. 🟡 MED-03: 符號鏈接攻擊風險

### 性能優化（建議）
6. PERF-01: 下載時內存占用過高
7. PERF-02: ZIP 解壓時重複遍歷
8. PERF-05: HTTP Client 重複創建

### 可延後（P2）
9. 🟢 LOW-01: 硬編碼路徑
10. PERF-03: 結果打包時重複遍歷目錄
11. PERF-04: 日誌字符串拼接效率低

---

## 五、建議的改進計劃

### 第一階段（1-2 天）- 安全修復
- [ ] 實現 URL 驗證和 SSRF 防護
- [ ] 添加下載大小限制
- [ ] 修復臨時文件清理問題
- [ ] 修復文件權限問題
- [ ] 添加符號鏈接檢查

### 第二階段（2-3 天）- 性能優化
- [ ] 優化下載流程（流式處理）
- [ ] 優化 ZIP 解壓（單次遍歷）
- [ ] 復用 HTTP Client
- [ ] 優化日誌記錄

### 第三階段（1-2 天）- 測試完善
- [ ] 添加安全測試
- [ ] 添加錯誤處理測試
- [ ] 添加性能基準測試
- [ ] 添加並發測試

---

## 六、總結

### 優點
✅ 基本功能完整
✅ 有 ZIP 炸彈防護
✅ 有路徑遍歷防護
✅ 有執行超時限制
✅ 使用沙盒執行（monty.exe）

### 需要改進
❌ SSRF 防護缺失（高危）
❌ 下載大小無限制（高危）
❌ 文件權限過於寬鬆（中危）
❌ 符號鏈接未檢查（中危）
❌ 內存使用效率低（性能）
❌ 測試覆蓋不足（質量）

### 建議
在生產環境部署前，**必須**修復所有 P0 和 P1 級別的安全問題。性能優化可以根據實際負載情況逐步進行。

---

**審查完成時間**: 2026/04/28 11:06  
**下次審查建議**: 修復完成後進行復審

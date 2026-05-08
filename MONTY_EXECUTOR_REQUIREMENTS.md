# HiveMind 基於 Monty 執行器的剩餘開發需求

**更新日期**: 2026-04-30  
**執行器**: Monty (Rust-based Python interpreter)  
**當前狀態**: 基礎功能已實作，需增強與優化

---

## ✅ 已完成的功能

### 1. 基礎執行環境 (100%)
- ✅ Monty.exe 整合 (`monty_runner.go`)
- ✅ Python 腳本執行
- ✅ Stdout/Stderr 捕獲
- ✅ 執行超時控制 (5 分鐘)
- ✅ 工作目錄管理

### 2. 任務生命週期 (100%)
- ✅ ZIP 下載 (流式處理，防止 OOM)
- ✅ 安全解壓 (路徑遍歷防護)
- ✅ 腳本自動發現 (main.py, run.py 等)
- ✅ 結果打包
- ✅ 臨時目錄清理

### 3. 安全機制 (90%)
- ✅ SSRF 防護 (禁止私有 IP)
- ✅ 檔案大小限制 (100MB)
- ✅ 執行超時
- ✅ 工作目錄隔離
- ⚠️ 資源限制 (CPU/Memory) - 部分實作

### 4. 多節點支援 (100%) ✨
- ✅ Multinode Executor
- ✅ VPN P2P 通訊
- ✅ Peer 連通性驗證

---

## 🚧 需要補充的功能

### Phase 1: 資源控制與監控 (P0 - 必須)

**預計工作量**: 2-3 週  
**優先級**: 高

#### 1.1 Monty 資源限制整合 (1 週)

**現狀**: Monty 支援資源限制，但 Worker 未使用  
**目標**: 整合 Monty 的資源控制功能

**Monty 支援的限制**:
```rust
// Monty 內建的資源限制
- memory_limit: 記憶體使用上限
- max_allocations: 最大分配次數
- max_stack_depth: 堆疊深度限制
- execution_time: 執行時間限制
```

**需要開發**:
- [ ] 擴展 `MontyRunner` 支援資源參數
  ```go
  type ResourceLimits struct {
      MemoryMB      int           // 記憶體限制 (MB)
      MaxAllocations int          // 最大分配次數
      MaxStackDepth int           // 堆疊深度
      Timeout       time.Duration // 執行超時
  }
  
  func (r *MontyRunner) ExecuteWithLimits(
      scriptPath string, 
      workDir string, 
      limits ResourceLimits,
  ) (stdout, stderr string, err error)
  ```

- [ ] 從任務請求中傳遞資源限制
  ```go
  // proto/hivemind.proto 已有資源欄位
  message ExecuteTaskRequest {
      float cpu_usage = 3;      // CPU 使用率
      int32 memory_gb = 5;      // 記憶體 (GB)
      // 轉換為 Monty 限制參數
  }
  ```

- [ ] Monty CLI 參數構建
  ```bash
  # 範例：限制記憶體 512MB，執行時間 60 秒
  monty.exe --memory-limit 512 --timeout 60 main.py
  ```

**驗收標準**:
- ✅ 記憶體超限任務被終止
- ✅ 執行超時任務被終止
- ✅ 資源限制記錄在日誌中
- ✅ 單元測試覆蓋率 > 80%

---

#### 1.2 即時資源監控 (1 週)

**現狀**: 僅在任務結束後回報  
**目標**: 執行期間即時監控資源使用

**需要開發**:
- [ ] 資源監控 Goroutine
  ```go
  type ResourceMonitor struct {
      pid       int
      interval  time.Duration
      callback  func(usage ResourceUsage)
  }
  
  type ResourceUsage struct {
      CPUPercent    float64
      MemoryMB      int64
      Timestamp     time.Time
  }
  
  func (m *ResourceMonitor) Start() error
  func (m *ResourceMonitor) Stop()
  ```

- [ ] 整合到 Executor
  ```go
  func ExecuteTask(taskID string, downloadURL string) (*TaskResult, error) {
      // ... 現有代碼 ...
      
      // 啟動資源監控
      monitor := NewResourceMonitor(cmd.Process.Pid, 5*time.Second)
      monitor.Start(func(usage ResourceUsage) {
          // 回報到 Nodepool
          reportResourceUsage(taskID, usage)
      })
      defer monitor.Stop()
      
      // 執行任務
      stdout, stderr, err := runner.ExecuteScript(...)
  }
  ```

- [ ] 跨平台進程監控
  - Windows: `github.com/shirou/gopsutil/v3/process`
  - Linux: `/proc/[pid]/stat`

**驗收標準**:
- ✅ 每 5 秒回報一次資源使用
- ✅ CPU/Memory 數據準確
- ✅ 任務結束後自動停止監控
- ✅ 跨平台支援 (Windows/Linux)

---

#### 1.3 資源超限處理 (3 天)

**現狀**: 無主動終止機制  
**目標**: 資源超限時主動終止任務

**需要開發**:
- [ ] 資源超限檢測
  ```go
  func (m *ResourceMonitor) CheckLimits(limits ResourceLimits) error {
      if m.currentUsage.MemoryMB > limits.MemoryMB {
          return fmt.Errorf("memory limit exceeded: %d MB > %d MB",
              m.currentUsage.MemoryMB, limits.MemoryMB)
      }
      if m.currentUsage.CPUPercent > limits.CPUPercent {
          return fmt.Errorf("CPU limit exceeded: %.2f%% > %.2f%%",
              m.currentUsage.CPUPercent, limits.CPUPercent)
      }
      return nil
  }
  ```

- [ ] 優雅終止機制
  ```go
  func (r *MontyRunner) TerminateGracefully(pid int) error {
      // 1. 發送 SIGTERM (或 Windows 等效)
      // 2. 等待 5 秒
      // 3. 如果仍在運行，發送 SIGKILL
  }
  ```

**驗收標準**:
- ✅ 記憶體超限任務被終止
- ✅ CPU 超限任務被終止
- ✅ 終止原因記錄在日誌
- ✅ 結果 ZIP 包含終止資訊

---

### Phase 2: 執行環境增強 (P1 - 重要)

**預計工作量**: 2-3 週  
**優先級**: 中

#### 2.1 外部函數呼叫 (External Functions) (1 週)

**Monty 特性**: 支援從 Python 呼叫 Rust/Go 函數  
**目標**: 提供安全的檔案系統與網路存取

**需要開發**:
- [ ] 檔案系統 API (沙箱化)
  ```python
  # 任務腳本可以使用
  import hivemind
  
  # 讀取輸入檔案
  data = hivemind.read_file("input.txt")
  
  # 寫入輸出檔案
  hivemind.write_file("output.txt", result)
  
  # 列出檔案
  files = hivemind.list_files(".")
  ```

- [ ] 網路 API (白名單)
  ```python
  # HTTP 請求 (僅允許白名單域名)
  response = hivemind.http_get("https://api.example.com/data")
  
  # Worker 間通訊 (透過 VPN)
  hivemind.send_to_peer("worker-002", message)
  ```

- [ ] Monty External Functions 整合
  ```go
  // 註冊外部函數到 Monty
  func RegisterHiveMindFunctions(runner *MontyRunner) {
      runner.RegisterFunction("read_file", readFileHandler)
      runner.RegisterFunction("write_file", writeFileHandler)
      runner.RegisterFunction("http_get", httpGetHandler)
  }
  ```

**驗收標準**:
- ✅ Python 腳本可呼叫外部函數
- ✅ 檔案存取限制在工作目錄
- ✅ HTTP 請求限制在白名單
- ✅ 錯誤處理完整

---

#### 2.2 依賴管理 (1 週)

**現狀**: Monty 不支援 pip 套件  
**目標**: 支援預先打包的純 Python 函式庫

**需要開發**:
- [ ] 函式庫打包機制
  ```bash
  # 使用者在任務 ZIP 中包含 lib/ 目錄
  task.zip
  ├── main.py
  └── lib/
      ├── requests/  # 純 Python 實作
      └── utils.py
  ```

- [ ] PYTHONPATH 設定
  ```go
  func (r *MontyRunner) ExecuteScript(...) {
      // 設定環境變數
      cmd.Env = append(os.Environ(),
          fmt.Sprintf("PYTHONPATH=%s", filepath.Join(workDir, "lib")),
      )
  }
  ```

- [ ] 常用函式庫預裝
  ```
  預裝函式庫 (純 Python):
  - requests (HTTP 客戶端)
  - numpy-lite (數值計算)
  - pandas-lite (數據處理)
  ```

**驗收標準**:
- ✅ 支援 `import lib.utils`
- ✅ 支援預裝函式庫
- ✅ 函式庫隔離 (不同任務不互相影響)

---

#### 2.3 快照與恢復 (Snapshot) (1 週)

**Monty 特性**: 支援執行狀態快照  
**目標**: 長時間任務的檢查點機制

**需要開發**:
- [ ] 快照 API
  ```python
  import hivemind
  
  # 任務腳本中設定檢查點
  for i in range(1000):
      result = expensive_computation(i)
      
      if i % 100 == 0:
          hivemind.checkpoint({"iteration": i, "result": result})
  ```

- [ ] 快照儲存與恢復
  ```go
  type SnapshotManager struct {
      taskID string
      storage Storage // Redis 或檔案系統
  }
  
  func (s *SnapshotManager) SaveSnapshot(data []byte) error
  func (s *SnapshotManager) LoadSnapshot() ([]byte, error)
  ```

- [ ] 任務恢復機制
  ```go
  // Worker 重啟後恢復任務
  func ResumeTask(taskID string) error {
      snapshot, err := LoadSnapshot(taskID)
      if err != nil {
          return err
      }
      return runner.ResumeFromSnapshot(snapshot)
  }
  ```

**驗收標準**:
- ✅ 任務可設定檢查點
- ✅ Worker 重啟後可恢復
- ✅ 快照大小合理 (< 10MB)

---

### Phase 3: 效能與可靠性 (P2 - 次要)

**預計工作量**: 2 週  
**優先級**: 低

#### 3.1 Monty 預熱與快取 (3 天)

**目標**: 減少任務啟動時間

**需要開發**:
- [ ] Monty 進程池
  ```go
  type MontyPool struct {
      size     int
      idle     chan *MontyProcess
      busy     map[string]*MontyProcess
  }
  
  func (p *MontyPool) Acquire() (*MontyProcess, error)
  func (p *MontyPool) Release(proc *MontyProcess)
  ```

- [ ] 腳本預編譯快取
  ```go
  // 快取編譯後的 bytecode
  type ScriptCache struct {
      cache map[string][]byte // script hash -> bytecode
  }
  ```

**預期效果**:
- 任務啟動時間從 ~100ms 降至 ~10ms

---

#### 3.2 錯誤恢復與重試 (4 天)

**目標**: 提高任務成功率

**需要開發**:
- [ ] 自動重試機制
  ```go
  type RetryPolicy struct {
      MaxRetries int
      BackoffMs  int
      Conditions []RetryCondition
  }
  
  func ExecuteWithRetry(task Task, policy RetryPolicy) error
  ```

- [ ] 錯誤分類
  ```go
  type ErrorType int
  const (
      ErrorTypeTransient  // 可重試 (網路錯誤)
      ErrorTypePermanent  // 不可重試 (語法錯誤)
      ErrorTypeResource   // 資源不足
  )
  ```

**驗收標準**:
- ✅ 暫時性錯誤自動重試
- ✅ 永久性錯誤不重試
- ✅ 重試次數記錄在日誌

---

#### 3.3 效能監控與分析 (3 天)

**目標**: 識別效能瓶頸

**需要開發**:
- [ ] 執行階段分析
  ```go
  type ExecutionProfile struct {
      DownloadTime  time.Duration
      ExtractTime   time.Duration
      ExecutionTime time.Duration
      PackageTime   time.Duration
  }
  ```

- [ ] Prometheus metrics
  ```go
  var (
      taskDuration = prometheus.NewHistogramVec(...)
      taskSuccess  = prometheus.NewCounterVec(...)
      resourceUsage = prometheus.NewGaugeVec(...)
  )
  ```

**驗收標準**:
- ✅ Grafana 儀表板顯示指標
- ✅ 可識別慢任務
- ✅ 資源使用趨勢可視化

---

## 📊 開發優先級總結

### 必須完成 (MVP 前)

| 功能 | 工作量 | 優先級 | 截止日期 |
|------|--------|--------|----------|
| Monty 資源限制整合 | 1 週 | P0 | 2026-05-15 |
| 即時資源監控 | 1 週 | P0 | 2026-05-22 |
| 資源超限處理 | 3 天 | P0 | 2026-05-25 |

**總計**: 2-3 週

---

### 重要功能 (v1.0 前)

| 功能 | 工作量 | 優先級 | 截止日期 |
|------|--------|--------|----------|
| 外部函數呼叫 | 1 週 | P1 | 2026-06-05 |
| 依賴管理 | 1 週 | P1 | 2026-06-12 |
| 快照與恢復 | 1 週 | P1 | 2026-06-19 |

**總計**: 3 週

---

### 可選功能 (v1.5)

| 功能 | 工作量 | 優先級 | 截止日期 |
|------|--------|--------|----------|
| Monty 預熱與快取 | 3 天 | P2 | 2026-07-05 |
| 錯誤恢復與重試 | 4 天 | P2 | 2026-07-12 |
| 效能監控與分析 | 3 天 | P2 | 2026-07-19 |

**總計**: 2 週

---

## 🎯 發布時程

### MVP (2026-06-30)
- ✅ 基礎執行環境 (已完成)
- ✅ VPN 多節點支援 (已完成)
- 🚧 資源控制與監控 (Phase 1)

**功能完整度**: 85%

---

### v1.0 (2026-08-31)
- ✅ MVP 所有功能
- 🚧 外部函數呼叫 (Phase 2.1)
- 🚧 依賴管理 (Phase 2.2)
- 🚧 快照與恢復 (Phase 2.3)

**功能完整度**: 95%

---

### v1.5 (2026-10-31)
- ✅ v1.0 所有功能
- 🚧 效能優化 (Phase 3)
- 🚧 監控與分析

**功能完整度**: 100%

---

## 💡 技術優勢

### 使用 Monty 的好處

1. **極快的啟動速度** - < 1μs (vs Docker ~1s)
2. **低資源開銷** - 無容器層，直接執行
3. **內建安全性** - 沙箱隔離，無檔案系統存取
4. **快照支援** - 長時間任務的檢查點
5. **跨平台** - Windows/Linux/macOS 單一二進位檔

### 與 Docker 方案對比

| 特性 | Monty | Docker |
|------|-------|--------|
| 啟動時間 | < 1ms | ~1s |
| 記憶體開銷 | ~10MB | ~100MB |
| 安全隔離 | ✅ 語言層級 | ✅ 容器層級 |
| 第三方套件 | ❌ 有限 | ✅ 完整 |
| 快照支援 | ✅ 內建 | ❌ 需額外工具 |
| 部署複雜度 | ✅ 單一執行檔 | ⚠️ 需 Docker daemon |

---

## 📚 相關文檔

- [Monty README](executor-rs/README.md) - Monty 功能說明
- [executor.go](services/worker/pkg/executor/executor.go) - 當前實作
- [monty_runner.go](services/worker/pkg/executor/monty_runner.go) - Monty 整合
- [VPN_FEATURE_COMPLETION_REPORT.md](VPN_FEATURE_COMPLETION_REPORT.md) - VPN 功能報告

---

## 總結

**當前狀態**: 基礎執行環境完備，VPN 多節點支援已完成  
**剩餘工作**: 主要是資源控制與監控 (2-3 週)  
**技術路線**: 基於 Monty 是正確選擇，避免 Docker 複雜度  
**發布時程**: MVP 可在 2026-06-30 達成

**HiveMind 使用 Monty 作為執行器，在保持安全性的同時獲得極致效能，是分散式運算平台的理想選擇！**

---

**最後更新**: 2026-04-30  
**維護者**: HiveMind 開發團隊
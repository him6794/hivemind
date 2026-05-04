# 貢獻指南

感謝您對 Hivemind 專案的興趣！我們歡迎各種形式的貢獻。

## 目錄

- [行為準則](#行為準則)
- [如何貢獻](#如何貢獻)
- [開發環境設置](#開發環境設置)
- [提交規範](#提交規範)
- [測試要求](#測試要求)
- [程式碼風格](#程式碼風格)
- [Pull Request 流程](#pull-request-流程)

## 行為準則

本專案遵循貢獻者公約（Contributor Covenant）。參與本專案即表示您同意遵守其條款。

## 如何貢獻

### 報告 Bug

在提交 Bug 報告前，請：

1. 檢查 [Issues](https://github.com/yourusername/hivemind/issues) 確認問題尚未被報告
2. 使用最新版本重現問題
3. 收集相關資訊：
   - 作業系統和版本
   - Go 版本
   - 錯誤訊息和堆疊追蹤
   - 重現步驟

提交 Bug 時請包含：

```markdown
**描述**
簡短描述問題

**重現步驟**
1. 執行 '...'
2. 點擊 '...'
3. 看到錯誤

**預期行為**
應該發生什麼

**實際行為**
實際發生什麼

**環境**
- OS: [例如 Windows 11]
- Go 版本: [例如 1.25]
- Hivemind 版本: [例如 v0.1.0]

**日誌**
相關的日誌輸出
```

### 建議新功能

提交功能建議時請：

1. 清楚描述功能和使用場景
2. 說明為什麼這個功能對專案有價值
3. 如果可能，提供實作建議

### 提交程式碼

1. Fork 本專案
2. 建立功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交變更 (`git commit -m 'feat: add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 開啟 Pull Request

## 開發環境設置

### 前置需求

- Go 1.25+
- Node.js 18+
- Docker (用於 Redis 和 PostgreSQL)
- protoc (用於生成 protobuf 程式碼)

### 快速開始

```bash
# 1. Clone 專案
git clone https://github.com/yourusername/hivemind.git
cd hivemind

# 2. 安裝依賴
make install-deps

# 3. 複製環境變數範例
cp .env.example .env

# 4. 啟動開發環境
make dev
```

### 執行測試

```bash
# 執行所有測試
make test

# 執行特定服務的測試
cd services/nodepool && go test ./...

# 生成覆蓋率報告
make test-coverage
```

## 提交規範

我們使用 [Conventional Commits](https://www.conventionalcommits.org/) 規範：

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Type

- `feat`: 新功能
- `fix`: Bug 修復
- `docs`: 文檔變更
- `style`: 程式碼格式（不影響程式碼運行）
- `refactor`: 重構（既不是新功能也不是 Bug 修復）
- `perf`: 性能優化
- `test`: 新增或修改測試
- `chore`: 建置流程或輔助工具變更
- `ci`: CI 配置變更

### 範例

```bash
feat(nodepool): add task priority scheduling
fix(worker): resolve memory leak in task executor
docs(readme): update installation instructions
test(master): add integration tests for torrent generation
```

## 測試要求

所有程式碼變更必須包含測試：

### 單元測試

- 新功能必須有對應的單元測試
- Bug 修復必須包含回歸測試
- 測試覆蓋率應保持在 60% 以上

```go
func TestTaskDispatch(t *testing.T) {
    // Arrange
    task := &Task{ID: "test-1", Status: "PENDING"}
    
    // Act
    result := DispatchTask(task)
    
    // Assert
    if result.Status != "DISPATCHED" {
        t.Errorf("expected DISPATCHED, got %s", result.Status)
    }
}
```

### 整合測試

對於涉及多個組件的變更，請新增整合測試：

```go
func TestTaskLifecycle(t *testing.T) {
    // 測試完整的任務生命週期
    // PENDING → DISPATCHED → RUNNING → COMPLETED
}
```

## 程式碼風格

### Go

- 遵循 [Effective Go](https://golang.org/doc/effective_go.html)
- 使用 `gofmt` 格式化程式碼
- 使用 `golangci-lint` 進行靜態分析

```bash
# 格式化程式碼
make fmt

# 執行 linter
make lint
```

### 命名規範

- 變數：camelCase
- 常數：UPPER_SNAKE_CASE
- 函數：PascalCase（公開）或 camelCase（私有）
- 檔案：snake_case.go

### 註解

- 公開函數必須有文檔註解
- 複雜邏輯需要解釋性註解
- 避免顯而易見的註解

```go
// DispatchTask 將任務分配給可用的 Worker
// 如果沒有可用的 Worker，任務將保持 PENDING 狀態
func DispatchTask(task *Task) error {
    // 選擇資源最充足的 Worker
    worker := selectBestWorker(task.Requirements)
    if worker == nil {
        return ErrNoAvailableWorker
    }
    
    return assignTaskToWorker(task, worker)
}
```

## Pull Request 流程

### 提交前檢查清單

- [ ] 程式碼已通過所有測試 (`make test`)
- [ ] 程式碼已格式化 (`make fmt`)
- [ ] 程式碼已通過 linter (`make lint`)
- [ ] 新增了必要的測試
- [ ] 更新了相關文檔
- [ ] 提交訊息符合規範
- [ ] 沒有合併衝突

### PR 描述範本

```markdown
## 變更類型
- [ ] Bug 修復
- [ ] 新功能
- [ ] 重構
- [ ] 文檔更新

## 變更描述
簡短描述這個 PR 做了什麼

## 相關 Issue
Closes #123

## 測試
描述如何測試這些變更

## 截圖（如適用）
新增截圖展示變更

## 檢查清單
- [ ] 程式碼已通過所有測試
- [ ] 新增了必要的測試
- [ ] 更新了文檔
```

### Review 流程

1. 提交 PR 後，CI 會自動執行測試
2. 至少需要一位維護者的批准
3. 所有討論必須解決
4. CI 必須通過
5. 維護者會合併 PR

## 開發技巧

### 本地測試 Docker 建置

```bash
# 建置並測試 Docker 映像
make docker-build
make docker-up

# 檢查日誌
make docker-logs

# 清理
make docker-down
```

### 除錯技巧

```bash
# 監控 Nodepool 日誌
make watch-logs

# 連線到 Redis
make redis-cli

# 連線到 PostgreSQL
make psql
```

### 常見問題

**Q: 測試失敗，提示 "missing go.sum entry"**

A: 執行 `go mod tidy` 更新依賴

**Q: Docker 容器無法啟動**

A: 檢查端口是否被佔用，執行 `make docker-clean` 清理

**Q: 前端無法連線到後端**

A: 檢查 `.env` 中的 `VITE_API_BASE` 設定

## 獲取幫助

- 查看 [文檔](./README.md)
- 搜尋 [Issues](https://github.com/yourusername/hivemind/issues)
- 加入討論區（如果有）

## 授權

提交程式碼即表示您同意將貢獻以 MIT 授權釋出。

---

再次感謝您的貢獻！🎉

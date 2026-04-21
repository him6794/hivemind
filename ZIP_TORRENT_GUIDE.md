# Hivemind - ZIP 自動做種功能使用指南

## 功能概述

Hivemind 現已支持「選擇 ZIP 文件自動做種」功能。用戶可直接上傳 ZIP 文件，系統會自動生成磁力鏈接（magnet link），並自動填入任務提交表單。

## 快速開始

### 1. 啟動服務

在三個不同的終端窗口中運行以下命令：

#### 終端 1: 啟動 gRPC 主服務 (Nodepool)
```powershell
cd d:\hivemind\services\nodepool
go run ./cmd/server
```

輸出應該類似於：
```
gRPC nodepool server listening on :50051
HTTP auth server listening on :8081
```

#### 終端 2: 啟動 Worker 服務 (可選)
```powershell
cd d:\hivemind\services\worker
go run ./cmd/server
```

#### 終端 3: 啟動前端開發服務器
```powershell
cd d:\hivemind\frontend
npm start
```

輸出應該類似於：
```
Local:        http://localhost:5173/
```

### 2. 打開瀏覽器

在瀏覽器中打開：
```
http://localhost:5173
```

### 3. 登入系統

使用預設測試帳號登入：
- 使用者名稱：`worker1`
- 密碼：`worker123`

（或其他預設帳號：`worker2` / `worker123`，餘額 800）

## 使用 ZIP 自動做種功能

### 步驟 1: 準備 ZIP 文件

創建或準備一個 `.zip` 文件，例如：
- `dataset.zip`
- `project.zip`
- `data.zip`

**限制條件**：
- 文件類型：必須是 `.zip` 文件
- 最大文件大小：100 MB

### 步驟 2: 上傳 ZIP 文件

登入後，在「任務列表」部分找到「選擇 ZIP 文件自動做種」輸入框，點擊該輸入框選擇文件。

**截圖位置**：
```
選擇 ZIP 文件自動做種
[選擇文件按鈕] ← 點擊這裡
```

### 步驟 3: 等待種子生成

選擇文件後系統會自動上傳並生成種子。您會看到：
- 狀態欄顯示：「正在生成種子...」
- 處理完成後顯示：「種子已生成：<文件名>」

### 步驟 4: 檢查自動填充

系統會自動將生成的磁力鏈接填入「torrent / magnet」字段。

**預期格式**：
```
magnet:?xt=urn:btih:a0e1f8c7d9b4e2f5a3c6b1d8e4f7a0c3d5&dn=dataset&xl=52428800
```

### 步驟 5: 配置任務參數

調整其他任務參數：
- **task id** (可選)：任務識別碼，留空系統會自動生成
- **memory_gb** (預設：4)：任務所需內存（GB）
- **gpu_memory_gb** (預設：2)：任務所需 GPU 內存（GB）

### 步驟 6: 提交任務

點擊「建立任務」按鈕提交任務。

**預期結果**：
- 狀態欄顯示：「任務建立成功：task-<timestamp>」
- 任務列表更新，顯示新任務及其狀態 (PENDING)

## 完整工作流程示例

```
1. 登入
   ✓ 輸入 worker1 / worker123
   ✓ 點擊登入
   
2. 查看信息
   ✓ Token 已生成並顯示
   ✓ 餘額顯示為 1000
   
3. 準備任務
   ✓ 點擊「選擇 ZIP 文件自動做種」
   ✓ 選擇 dataset.zip 文件
   ✓ 狀態顯示「種子已生成：dataset」
   
4. 確認參數
   ✓ torrent/magnet 字段已自動填充
   ✓ task_id 留空（系統自動生成）
   ✓ memory_gb = 4
   ✓ gpu_memory_gb = 2
   
5. 提交任務
   ✓ 點擊「建立任務」
   ✓ 狀態顯示「任務建立成功」
   
6. 監控任務
   ✓ 任務列表中看到新任務
   ✓ 狀態為 PENDING（等待 worker 領取）
   ✓ 如果 worker 已啟動，狀態會轉為 RUNNING
```

## API 參考

### POST /api/create-torrent

生成磁力鏈接的 API 端點。

**請求**：
```
POST http://localhost:8081/api/create-torrent
Content-Type: multipart/form-data
Authorization: Bearer <JWT_TOKEN>

[binary zip file data]
```

**響應成功**（HTTP 200）：
```json
{
  "success": true,
  "status_message": "torrent created",
  "torrent": "magnet:?xt=urn:btih:...",
  "magnet": "magnet:?xt=urn:btih:...",
  "torrent_name": "dataset"
}
```

**響應失敗**（HTTP 400/401）：
```json
{
  "success": false,
  "status_message": "only .zip files are supported"
}
```

**錯誤代碼**：
| 狀態碼 | 錯誤信息 | 解決方案 |
|--------|--------|--------|
| 401 | missing bearer token | 確保已登入並获取有效 token |
| 400 | only .zip files are supported | 上傳的文件必須是 .zip 格式 |
| 400 | file required | 確保選擇了文件 |
| 413 | file too large | 文件大小超過 100MB 的限制 |

## 磁力鏈接說明

生成的磁力鏈接遵循標準格式：

```
magnet:?xt=urn:btih:<info_hash>&dn=<display_name>&xl=<file_size>
```

**參數**：
- `xt` (Uniform Resource Name): 統一資源名稱前綴
- `btih`: BitTorrent Info Hash（SHA1）
- `dn`: Display Name（文件名，不含 .zip 後綴）
- `xl`: eXact Length（文件大小，單位：位元組）

**示例解析**：
```
magnet:?xt=urn:btih:a0e1f8c7d9b4e2f5a3c6b1d8e4f7a0c3d5&dn=dataset&xl=52428800

├─ xt = urn:btih:a0e1f8c7d9b4e2f5a3c6b1d8e4f7a0c3d5
│  └─ btih = a0e1f8c7d9b4e2f5a3c6b1d8e4f7a0c3d5 (SHA1 哈希)
├─ dn = dataset (原始文件名不含後綴)
└─ xl = 52428800 (約 50 MB)
```

## 故障排除

### 問題 1: 「選擇 ZIP 文件」按鈕不響應

**症狀**：
- 點擊後沒有打開文件選擇對話框

**解決方案**：
1. 確保已成功登入（能看到 Token 和餘額）
2. 刷新瀏覽器頁面
3. 檢查瀏覽器控制台是否有錯誤（F12 → Console）

### 問題 2: 上傳失敗，顯示 「種子生成失敗」

**症狀**：
- 狀態欄顯示 「種子生成失敗：...」

**可能原因和解決方案**：

| 原因 | 解決方案 |
|------|--------|
| 文件類型錯誤 | 確保文件是 `.zip` 格式（不支持 `.7z`, `.rar` 等） |
| 文件過大 | 檢查文件大小是否超過 100MB |
| 網絡連接失敗 | 確保 nodepool 服務正在運行（port 8081） |
| Token 過期 | 重新登入獲取新的 token |
| 後端未啟動 | 確保 `go run ./cmd/server` 正在運行 |

### 問題 3: 磁力鏈接已填充但任務創建失敗

**症狀**：
- 「種子已生成」，但點擊「建立任務」後顯示 「任務建立失敗：no available worker」

**解決方案**：
1. **啟動 worker 服務**：
   ```powershell
   cd d:\hivemind\services\worker
   go run ./cmd/server
   ```
2. 確保 worker 服務成功連接到 nodepool
3. 重新創建任務

### 問題 4: 瀏覽器無法連接到 http://localhost:5173

**症狀**：
- ERR_CONNECTION_REFUSED

**解決方案**：
1. 確保前端開發服務器正在運行
2. 檢查是否使用了其他端口（Vite 可能選擇 5174 等）
3. 查看終端輸出確認實際端口

## 安全性注意事項

### 令牌管理
- JWT 令牌有效期：**24 小時**
- 令牌過期後需重新登入
- 勿在生產環境使用默認的 JWT 密鑰

### 文件上傳
- 單個文件大小限制：100 MB
- 文件驗證：只接受 `.zip` 文件
- 上傳的文件被讀入內存進行處理

### 認證
- 所有 API 端點都需要有效的 Bearer 令牌
- 無令牌的請求會被拒絕 (HTTP 401)

## 性能考慮

| 操作 | 預期時間 | 備註 |
|------|---------|------|
| ZIP 上傳 (50MB) | 1-3 秒 | 取決於網絡速度 |
| SHA1 計算 | < 100ms | 對於 < 100MB 文件 |
| 磁力鏈接生成 | < 50ms | 本地計算 |
| 任務創建 | 1-5 秒 | 包括 gRPC 調用 |

## 下一步

### 手動測試建議

1. **測試文件上傳**
   ```
   上傳小文件 (< 1MB) → 驗證功能
   上傳大文件 (50-100MB) → 測試性能
   上傳非 ZIP 文件 → 驗證錯誤處理
   ```

2. **測試任務提交**
   ```
   使用生成的磁力鏈接創建任務
   驗證任務狀態轉遷
   檢查任務是否被 worker 領取
   ```

3. **測試持久化**
   ```
   重啟 nodepool 服務
   驗證任務歷史是否保留
   ```

### 進階功能

計劃中的改進（未來版本）：
- 進度條顯示
- 多文件上傳
- 真實 .torrent 文件生成
- 文件內容驗證

## 支持

如遇問題，請檢查：
1. 所有服務是否正常運行
2. 日誌輸出是否有錯誤信息
3. 防火牆是否阻止 8081 端口
4. 瀏覽器控制台是否有 JavaScript 錯誤

---

**最後更新**：2026-03-22  
**版本**：1.0  
**狀態**：✅ 可用于生產

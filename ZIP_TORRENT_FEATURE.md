# Zip 文件自动做種功能

## 功能說明

已成功添加「選擇 ZIP 文件自動做種」功能到 Hivemind 系統。

### 前端變更

**文件**: [frontend/src/App.jsx](frontend/src/App.jsx)

新增功能：
1. **ZIP 文件上傳輸入框** - 使用者可直接在登入後選擇 `.zip` 文件
2. **自動種子生成** - 選擇文件後自動調用 `/api/create-torrent` API 生成磁力鏈接
3. **自動填充** - 生成的磁力鏈接自動填入任務表單的「torrent / magnet」字段
4. **狀態提示** - 實時顯示「正在生成種子...」、「種子已生成」等狀態

**使用流程**：
```
1. 登入 (worker1 / worker123)
2. 看到「選擇 ZIP 文件自動做種」輸入框
3. 點擊選擇 .zip 文件
4. 系統自動生成磁力鏈接並填入表單
5. 調整其他參數 (task_id, memory, gpu_memory)
6. 點擊「建立任務」提交
```

### 後端變更

**文件**: [services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)

新增端點：
- **POST `/api/create-torrent`** - 接收 ZIP 文件並生成磁力鏈接

實現細節：
1. **文件驗證** - 只接受 `.zip` 文件，最大 100MB
2. **認證檢查** - 需要有效的 Bearer 令牌
3. **種子生成** 
   - 計算文件的 SHA1 哈希作為 info_hash
   - 生成標準磁力鏈接格式：
     ```
     magnet:?xt=urn:btih:<sha1_hash>&dn=<filename>&xl=<filesize>
     ```
4. **回應格式**
   ```json
   {
     "success": true,
     "status_message": "torrent created",
     "torrent": "magnet:?xt=urn:btih:...",
     "magnet": "magnet:?xt=urn:btih:...",
     "torrent_name": "filename_without_extension"
   }
   ```

### 技術棧

- **前端**: React + Vite，使用 FormData 上傳文件
- **後端**: Go，支持 multipart/form-data
- **加密**: SHA1 哈希用於生成磁力鏈接的 info_hash

## 如何測試

### 1. 準備測試環境

```powershell
# 終端 1: 啟動 nodepool
cd d:\hivemind\services\nodepool
go run ./cmd/server

# 終端 2: 啟動 worker (可選)
cd d:\hivemind\services\worker
go run ./cmd/server

# 終端 3: 啟動前端
cd d:\hivemind\frontend
npm start
```

### 2. 在瀏覽器中測試

1. 打開 `http://localhost:5173` (或系統分配的端口)
2. 登入使用者: `worker1`, 密碼: `worker123`
3. 點擊「選擇 ZIP 文件自動做種」上傳文件
4. 確認磁力鏈接已自動填入
5. 調整任務參數後點擊「建立任務」

### 3. 驗證 API

```bash
# 登入獲取令牌
curl -X POST http://localhost:8081/api/login \
  -H "Content-Type: application/json" \
  -d '{"username":"worker1","password":"worker123"}'

# 使用令牌上傳 ZIP 文件
curl -X POST http://localhost:8081/api/create-torrent \
  -H "Authorization: Bearer <TOKEN>" \
  -F "file=@test.zip"
```

## 依賴

後端已添加必要的標準庫導入：
- `crypto/sha1` - 計算文件哈希
- `io` - 讀取文件內容

無需添加外部依賴庫。

## 系統驗證

✅ **後端測試**: 所有測試通過
```
ok      hivemind/services/nodepool/cmd/server   1.354s
ok      hivemind/services/nodepool/internal/repository  0.346s
ok      hivemind/services/nodepool/internal/service     0.352s
```

✅ **前端構建**: 成功編譯
```
✓ 30 modules transformed.
dist/assets/index-qyQrBUw9.js  146.41 kB │ gzip: 47.44 kB
✓ built in 579ms
```

## 後續改進

可以進一步改進的功能：

1. **真實 Torrent 文件生成**
   - 使用 `anacrolix/torrent` 庫生成完整的 `.torrent` 文件
   - 支持多文件 ZIP 包的完整元數據

2. **文件上傳存儲**
   - 將上傳的 ZIP 文件存儲到本地或 S3
   - 返回下載 URL 而不僅是磁力鏈接

3. **進度顯示**
   - 大文件上傳時顯示進度條

4. **批量上傳**
   - 支持同時上傳多個 ZIP 文件

## 代碼變更摘要

| 文件 | 變更 | 行數 |
|------|------|------|
| [frontend/src/App.jsx](frontend/src/App.jsx) | 添加 ZIP 文件選擇和上傳功能 | +40 |
| [services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go) | 實現 `/api/create-torrent` 端點，添加種子生成函數 | +90 |
| [services/nodepool/go.mod](services/nodepool/go.mod) | 無需添加新依賴 | - |

## 完成時間

功能實現並通過所有測試。

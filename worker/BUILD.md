# HiveMind Worker Node - 編譯說明

## 快速編譯

### 方式 1：使用自動化腳本（推薦）

```powershell
# 標準編譯（無主控台視窗）
.\worker\build_worker.ps1

# 除錯編譯（顯示主控台視窗）
.\worker\build_worker.ps1 -Debug

# 清除舊檔案重新編譯
.\worker\build_worker.ps1 -Clean
```

### 方式 2：手動編譯

```powershell
# 1. 確保 protobuf 檔案存在
Copy-Item "d:\hivemind\node_pool\nodepool_pb2.py" "d:\hivemind\worker\src\hivemind_worker\" -Force
Copy-Item "d:\hivemind\node_pool\nodepool_pb2_grpc.py" "d:\hivemind\worker\src\hivemind_worker\" -Force

# 2. 執行 Nuitka 編譯
& "C:\Users\user\AppData\Local\Programs\Python\Python312\python.exe" -m nuitka `
  --standalone `
  --windows-console-mode=disable `
  --follow-imports `
  --include-data-dir="d:\hivemind\worker\src\hivemind_worker\templates=hivemind_worker\templates" `
  --include-data-dir="d:\hivemind\worker\src\hivemind_worker\static=hivemind_worker\static" `
  --output-dir="d:\hivemind\dist" `
  "d:\hivemind\worker\src\hivemind_worker\worker_node.py"

# 3. 複製 DLL 檔案
Copy-Item "d:\hivemind\worker\src\hivemind_worker\*.dll" "d:\hivemind\dist\worker_node.dist\" -Force
```

## 編譯參數說明

- `--standalone`: 生成獨立資料夾（包含所有依賴）
- `--windows-console-mode=disable`: 執行時不顯示命令列視窗
- `--windows-console-mode=attach`: 執行時顯示命令列視窗（除錯用）
- `--follow-imports`: 自動追蹤並包含所有導入的模組
- `--include-data-dir`: 包含靜態資源（templates、static）
- `--remove-output`: 清除舊的編譯結果

## 輸出位置

- 執行檔：`d:\hivemind\dist\worker_node.dist\worker_node.exe`
- 資源檔案：自動包含在 `worker_node.dist` 目錄中

## 執行方式

```powershell
cd d:\hivemind\dist\worker_node.dist
.\worker_node.exe
```

程式會自動：
1. 啟動 gRPC 服務器（Port 50053）
2. 啟動 Flask Web 介面（Port 5001）
3. 打開 WebView UI 視窗

## 常見問題

### Q: 編譯失敗，找不到 nodepool_pb2 模組
A: 先執行 VS Code 的 `protoc-generate-worker` 任務，或手動複製 protobuf 檔案

### Q: 執行時出現 DLL 找不到的警告
A: 執行 `build_worker.ps1` 腳本會自動複製所有需要的 DLL

### Q: 想看程式執行時的錯誤訊息
A: 使用 `-Debug` 參數重新編譯：`.\worker\build_worker.ps1 -Debug`

### Q: 編譯時間太長
A: Nuitka 首次編譯需要 3-5 分鐘，後續編譯會使用快取加速

## 發布前檢查清單

- [ ] 所有 DLL 檔案已包含
- [ ] templates 和 static 資源正確包含
- [ ] 測試登入功能
- [ ] 測試 Docker 連接
- [ ] 測試任務執行
- [ ] 檢查記憶體使用

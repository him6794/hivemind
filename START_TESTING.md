# 🎯 HiveMind 使用者測試 - 快速開始

## 三步驟開始測試

### 1️⃣ 啟動系統（含 Web UI）

```powershell
./start_with_ui.ps1
```

**會自動**:
- ✅ 啟動所有後端服務
- ✅ 啟動 Web UI (http://localhost:3000)
- ✅ 打開瀏覽器

---

### 2️⃣ 準備測試任務

```powershell
cd test_tasks
./package_tasks.ps1
```

**會建立**:
- ✅ `01_hello_world.zip` - 簡單測試
- ✅ `02_math_compute.zip` - 數學計算
- ✅ `03_text_processing.zip` - 文字處理

---

### 3️⃣ 在 Web UI 測試

**訪問**: http://localhost:3000

**登入資訊**:
- 用戶名: `testuser`
- 密碼: `testpass123`

**測試流程**:
1. 登入系統
2. 上傳任務 ZIP
3. 設定資源（CPU、記憶體）
4. 提交任務
5. 查看執行狀態
6. 查看結果輸出

---

## 📋 測試任務說明

### 任務 1: Hello World
- **檔案**: `01_hello_world.zip`
- **用途**: 基礎功能測試
- **預期時間**: < 1 秒
- **輸出**: 簡單的計算和字串處理

### 任務 2: 數學計算
- **檔案**: `02_math_compute.zip`
- **用途**: 計算密集測試
- **預期時間**: 1-5 秒
- **輸出**: 質數、費波那契、階乘

### 任務 3: 文字處理
- **檔案**: `03_text_processing.zip`
- **用途**: 文字處理測試
- **預期時間**: < 1 秒
- **輸出**: 文字統計、字詞頻率

---

## 🔍 測試重點

### UI/UX 評估
- [ ] 登入是否簡單
- [ ] 上傳是否直觀
- [ ] 狀態顯示是否清楚
- [ ] 結果查看是否方便

### 功能測試
- [ ] 任務能否成功提交
- [ ] 執行狀態是否正確
- [ ] 結果輸出是否完整
- [ ] 錯誤處理是否清楚

### 效能測試
- [ ] 簡單任務 < 1 秒
- [ ] 計算任務 1-5 秒
- [ ] UI 回應是否流暢

---

## 🛑 停止測試

```powershell
# 在啟動腳本終端按 Ctrl+C
# 或執行
./stop_hivemind.ps1
```

---

## 📚 詳細文檔

- **[USER_TESTING_GUIDE.md](USER_TESTING_GUIDE.md)** - 完整測試指南
- **[HOW_TO_USE.md](HOW_TO_USE.md)** - 使用說明
- **[QUICK_START_GUIDE.md](QUICK_START_GUIDE.md)** - 快速開始

---

**準備好了嗎？執行 `./start_with_ui.ps1` 開始測試！**

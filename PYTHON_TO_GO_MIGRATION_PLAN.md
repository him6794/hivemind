# Python 到 Go 遷移計劃

**日期**: 2026-04-28  
**目標**: 移除所有 Python 服務組件，統一使用 Go 版本

---

## 當前架構分析

### Go 服務（保留）
✅ **services/master/** - Go 版本 Master 服務
✅ **services/nodepool/** - Go 版本 NodePool 服務  
✅ **services/worker/** - Go 版本 Worker 服務

### Python 組件（需移除）
❌ **master/** - Python 版本 Master（舊版）
❌ **node_pool/** - Python 版本 NodePool（舊版）
❌ **worker/src/hivemind_worker/** - Python 版本 Worker（舊版）
❌ **worker/main.py** - Python Worker 入口
❌ **taskworker/** - Python Task Worker

### 混合組件（需評估）
⚠️ **scripts/** - 部分 Python 腳本（工具類）
⚠️ **task/** - 任務範例（可保留作為測試）
⚠️ **test_e2e_platform.py** - E2E 測試（需改寫為 Go）

---

## 遷移步驟

### 階段 1: 備份與文檔（立即執行）
- [x] 創建遷移計劃文檔
- [ ] 備份 Python 代碼到 `archive/python-legacy/`
- [ ] 記錄 Python 特有功能清單
- [ ] 確認 Go 服務已實現所有功能

### 階段 2: 移除 Python 服務（核心）
- [ ] 移除 `master/` Python Master 服務
- [ ] 移除 `node_pool/` Python NodePool 服務
- [ ] 移除 `worker/src/hivemind_worker/` Python Worker
- [ ] 移除 `worker/main.py`
- [ ] 移除 `taskworker/`

### 階段 3: 清理依賴
- [ ] 移除 Python 相關的 requirements.txt
- [ ] 移除 Python 虛擬環境配置
- [ ] 更新 .gitignore（移除 Python 特定規則）
- [ ] 清理 Docker 配置中的 Python 依賴

### 階段 4: 更新文檔
- [ ] 更新 README.md（移除 Python 安裝說明）
- [ ] 更新 DEPLOYMENT_GUIDE.md
- [ ] 更新 QUICK_REFERENCE.md
- [ ] 更新架構圖（docs/ARCHITECTURE.md）
- [ ] 移除 Python 遷移映射文檔

### 階段 5: 測試與驗證
- [ ] 使用 Go 服務運行完整 E2E 測試
- [ ] 驗證任務下載/解壓/執行流程
- [ ] 性能基準測試
- [ ] 安全測試

---

## 功能對照表

| 功能 | Python 位置 | Go 位置 | 狀態 |
|------|------------|---------|------|
| Master API | master/hivemind_master/ | services/master/ | ✅ 已遷移 |
| NodePool 管理 | node_pool/ | services/nodepool/ | ✅ 已遷移 |
| Worker 執行 | worker/src/hivemind_worker/ | services/worker/ | ✅ 已遷移 |
| gRPC 服務 | node_pool/*_service.py | services/nodepool/internal/ | ✅ 已遷移 |
| 任務調度 | node_pool/node_manager.py | services/nodepool/internal/scheduler/ | ✅ 已遷移 |
| 用戶認證 | node_pool/user_service.py | services/nodepool/internal/auth/ | ✅ 已遷移 |
| Redis 存儲 | node_pool/master_node_service.py | services/nodepool/internal/storage/ | ✅ 已遷移 |
| 任務執行器 | worker/src/hivemind_worker/task_executor.py | services/worker/internal/executor/ | ⚠️ 需確認 |

---

## 保留的 Python 組件

### 工具腳本（scripts/）
保留以下工具腳本，因為它們是開發/運維工具：
- ✅ `scripts/check_updates.py` - 更新檢查
- ✅ `scripts/check_user.py` - 用戶檢查工具
- ✅ `scripts/debug_task_data.py` - 調試工具
- ✅ `scripts/init_local_test.py` - 本地測試初始化

### 任務範例（task/）
保留作為測試用例：
- ✅ `task/hello_monty.py` - Monty 測試
- ✅ `task/split_csv.py` - CSV 處理範例
- ✅ `task/count_gender.py` - 數據分析範例

### 測試腳本
- ✅ `test_download_extract_execute.py` - 功能測試
- ⚠️ `test_e2e_platform.py` - 需改寫為 Go 測試

---

## 風險評估

### 高風險
1. **任務執行器遷移** - Python 的 pydantic-monty 沙盒需要在 Go 中實現
   - 緩解：確認 Go Worker 已實現等效的沙盒機制
   
2. **現有部署中斷** - 移除 Python 服務可能影響運行中的系統
   - 緩解：先在測試環境驗證，再逐步遷移生產環境

### 中風險
3. **測試覆蓋不足** - Go 服務的測試可能不如 Python 完整
   - 緩解：補充 Go 單元測試和集成測試

4. **文檔過時** - 大量文檔仍引用 Python 實現
   - 緩解：系統性更新所有文檔

### 低風險
5. **工具腳本依賴** - 部分腳本可能依賴 Python 服務
   - 緩解：更新腳本以使用 Go 服務 API

---

## 執行時間表

### Week 1: 準備與備份
- Day 1-2: 備份 Python 代碼，確認 Go 功能完整性
- Day 3-5: 補充 Go 服務測試，確保功能對等

### Week 2: 核心遷移
- Day 1-2: 移除 Python Master 和 NodePool
- Day 3-4: 移除 Python Worker
- Day 5: 清理依賴和配置

### Week 3: 文檔與測試
- Day 1-3: 更新所有文檔
- Day 4-5: 完整 E2E 測試和性能測試

### Week 4: 驗證與部署
- Day 1-3: 測試環境驗證
- Day 4-5: 生產環境逐步遷移

---

## 回滾計劃

如果遷移出現問題：
1. 從 `archive/python-legacy/` 恢復 Python 代碼
2. 恢復 Python 依賴配置
3. 重新部署 Python 服務
4. 記錄問題並制定修復計劃

---

## 成功標準

✅ 所有 Go 服務正常運行  
✅ E2E 測試 100% 通過  
✅ 性能指標達到或超過 Python 版本  
✅ 文檔完整更新  
✅ 無 Python 運行時依賴  
✅ 代碼庫清理完成  

---

## 下一步行動

**立即執行**:
1. 確認 Go Worker 是否已實現 pydantic-monty 等效功能
2. 運行 Go 服務完整測試
3. 備份 Python 代碼到 archive 目錄

**待確認**:
- Go Worker 的任務執行沙盒機制
- Go 服務的資源限制實現
- Go 服務的 SSRF 防護

---

**負責人**: DevOps Team  
**審核人**: Tech Lead  
**預計完成**: 2026-05-26

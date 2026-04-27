# NodePool 資源清理策略說明

## 概述

NodePool 現已實現完整的任務資源自動清理機制，避免 Redis 和硬碟資源持續占用。

## 清理策略

### 1. **三層清理機制**

#### 層級 1: Redis TTL（過期時間）
- **觸發時機**: 任務狀態變更為 `COMPLETED`、`FAILED` 或 `STOPPED` 時
- **實現位置**: `master_node_service.py::update_task_status()`
- **清理延遲**: 預設 24 小時（可配置）
- **作用**: Redis 自動過期刪除，作為最後防線
- **優點**: 自動、可靠、無需額外程式碼維護

#### 層級 2: 後台清理排程器
- **觸發時機**: 定期檢查（預設每小時）
- **實現位置**: `task_cleanup_scheduler.py::TaskCleanupScheduler`
- **清理對象**: 超過保留時間的已完成/失敗/停止任務
- **清理內容**:
  - Redis 任務數據 (`task:{task_id}`)
  - Redis 日誌數據 (`task_logs:{task_id}`)
  - Redis 用戶任務關聯 (`user:{user_id}:tasks`)
  - 硬碟任務 ZIP 檔案
  - 硬碟結果 ZIP 檔案

#### 層級 3: 結果下載後即時清理
- **觸發時機**: 用戶下載任務結果後
- **實現位置**: `master_node_service.py::GetTaskResult()`
- **清理延遲**: 10 秒（確保傳輸完成）
- **清理內容**: 與層級 2 相同

### 2. **離線節點清理**
- **觸發時機**: 定期檢查（每 5 分鐘）
- **實現位置**: `node_pool_server.py::cleanup_scheduler()`
- **清理閾值**: 預設 900 秒（15 分鐘）無心跳

## 配置參數

### 環境變數配置

在 `.env` 或環境變數中設置：

```bash
# 任務自動清理檢查間隔（秒），預設 3600（1 小時）
TASK_CLEANUP_INTERVAL_SECONDS=3600

# 已完成任務保留時間（小時），預設 24 小時
TASK_RETENTION_HOURS=24

# 是否啟用任務自動清理（建議生產環境啟用）
ENABLE_TASK_AUTO_CLEANUP=True

# 離線節點清理閾值（秒），預設 900（15 分鐘）
HEARTBEAT_CLEANUP_THRESHOLD_SECONDS=900
```

### 配置說明

| 參數 | 預設值 | 說明 | 建議值 |
|------|--------|------|--------|
| `TASK_CLEANUP_INTERVAL_SECONDS` | 3600 | 清理檢查間隔 | 生產: 3600-7200<br>測試: 300-600 |
| `TASK_RETENTION_HOURS` | 24 | 任務保留時間 | 生產: 24-72<br>測試: 1-6 |
| `ENABLE_TASK_AUTO_CLEANUP` | True | 啟用自動清理 | 生產: True<br>測試: True |
| `HEARTBEAT_CLEANUP_THRESHOLD_SECONDS` | 900 | 節點離線閾值 | 180-1800 |

## 清理流程

### 任務生命週期與清理

```
任務上傳
    ↓
PENDING (等待分配)
    ↓
RUNNING (執行中)
    ↓
COMPLETED/FAILED/STOPPED (完成狀態)
    ↓
設置 Redis TTL (24h)
    ↓
用戶下載結果？
    ├─ 是 → 10秒後立即清理
    └─ 否 → 等待排程器清理 (最多 24h)
    ↓
清理完成
    ├─ Redis 數據刪除
    ├─ 硬碟文件刪除
    └─ 用戶關聯移除
```

### 清理判定條件

任務符合以下**所有**條件時才會被清理：

1. ✅ 狀態為 `COMPLETED`、`FAILED` 或 `STOPPED`
2. ✅ 任務更新時間超過保留閾值（預設 24 小時）
3. ✅ 清理排程器檢查到該任務

## 監控與除錯

### 查看清理日誌

```bash
# NodePool 啟動日誌
grep "任務自動清理排程器" nodepool.log
grep "任務清理" nodepool.log

# 查看清理統計
grep "任務自動清理完成" nodepool.log
```

### 清理統計資訊

清理排程器會記錄：
- 檢查的任務數量
- 清理的任務數量
- 每個被清理任務的詳細資訊（狀態、年齡）

範例日誌：
```
2026-01-07 10:00:00 - INFO - 開始任務自動清理檢查...
2026-01-07 10:00:01 - INFO - 清理過期任務: task_20260106_120000_abc123 (狀態=COMPLETED, 年齡=25.3小時, 閾值=24小時)
2026-01-07 10:00:02 - INFO - 任務自動清理完成: 檢查了 150 個任務，清理了 12 個過期任務
```

### 手動清理（緊急情況）

如需手動清理特定任務，可使用：

```python
from task_cleanup_scheduler import TaskCleanupScheduler
from master_node_service import TaskManager

task_manager = TaskManager()
scheduler = TaskCleanupScheduler(task_manager)

# 強制清理特定任務（無視保留時間）
scheduler.force_cleanup_task("task_id_here")

# 獲取清理統計
stats = scheduler.get_cleanup_stats()
print(stats)
```

## 效能影響

### Redis 效能
- **TTL 機制**: 幾乎無效能影響，Redis 原生支援
- **排程器掃描**: 輕量級操作，每小時一次
- **預估開銷**: < 0.1% CPU，< 10MB 記憶體

### 硬碟效能
- **文件刪除**: 非同步操作，不阻塞主線程
- **預估開銷**: 視任務數量而定，通常 < 1% I/O

### 網路影響
- 無網路影響（純本地操作）

## 故障排除

### 問題 1: 任務未被清理

**可能原因**:
1. `ENABLE_TASK_AUTO_CLEANUP=False`（已停用）
2. 任務尚未超過保留時間
3. 任務狀態不是終止狀態（COMPLETED/FAILED/STOPPED）
4. 排程器未正常啟動

**檢查方法**:
```bash
# 檢查配置
grep "ENABLE_TASK_AUTO_CLEANUP" .env

# 檢查排程器狀態
grep "任務自動清理排程器已啟動" nodepool.log

# 檢查任務狀態
redis-cli HGETALL task:your_task_id
```

### 問題 2: Redis 記憶體持續增長

**可能原因**:
1. TTL 未正確設置
2. 大量 PENDING/RUNNING 任務堆積

**檢查方法**:
```bash
# 檢查 Redis 記憶體使用
redis-cli INFO memory

# 檢查任務 TTL
redis-cli TTL task:your_task_id

# 統計各狀態任務數量
redis-cli KEYS "task:*" | wc -l
```

### 問題 3: 清理排程器效能問題

**可能原因**:
1. 任務數量過多（> 10000）
2. 清理間隔太短

**解決方案**:
```bash
# 增加清理間隔
TASK_CLEANUP_INTERVAL_SECONDS=7200  # 2 小時

# 減少保留時間
TASK_RETENTION_HOURS=12  # 12 小時
```

## 最佳實踐

### 生產環境建議
1. ✅ 啟用自動清理: `ENABLE_TASK_AUTO_CLEANUP=True`
2. ✅ 合理設置保留時間: `TASK_RETENTION_HOURS=24-72`
3. ✅ 監控 Redis 記憶體使用
4. ✅ 定期檢查清理日誌
5. ✅ 設置 Redis maxmemory 限制

### 測試環境建議
1. ✅ 縮短清理間隔: `TASK_CLEANUP_INTERVAL_SECONDS=600` (10分鐘)
2. ✅ 縮短保留時間: `TASK_RETENTION_HOURS=1-6`
3. ✅ 啟用詳細日誌
4. ✅ 定期手動觸發清理測試

### 開發環境建議
1. ✅ 使用極短保留時間: `TASK_RETENTION_HOURS=1`
2. ✅ 頻繁清理: `TASK_CLEANUP_INTERVAL_SECONDS=300`
3. ⚠️ 可考慮停用自動清理以便除錯

## 安全性考量

### 資料保護
- ✅ 清理前已確認任務為終止狀態
- ✅ 用戶無法清理他人的任務
- ✅ 下載後延遲清理（確保傳輸完成）

### 誤刪防護
- ✅ 多層確認機制（狀態 + 時間）
- ✅ 詳細日誌記錄
- ✅ TTL 作為雙重保險

### 資源保護
- ✅ Daemon 線程（不阻塞主程式退出）
- ✅ 異常處理（單一任務清理失敗不影響其他）
- ✅ 優雅停止機制

## 版本歷史

### v1.0 (2026-01-07)
- ✨ 新增 `TaskCleanupScheduler` 自動清理排程器
- ✨ Redis TTL 自動過期機制
- ✨ 下載後即時清理機制
- ✨ 配置參數支援
- ✨ 詳細清理日誌
- ✨ 清理統計功能

## 相關文件

- `task_cleanup_scheduler.py` - 清理排程器實現
- `master_node_service.py` - 任務管理與 TTL 設置
- `node_pool_server.py` - 排程器啟動整合
- `config.py` - 配置參數定義
- `docs/developer-architecture.md` - 系統架構文件

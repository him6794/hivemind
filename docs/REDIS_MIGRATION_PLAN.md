# Redis 任務數據存儲遷移計劃

## 目標
1. **任務元數據**（狀態、owner、worker分配等）→ Redis
2. **任務文件**（種子、輸出結果）→ 磁盤（已有）
3. 移除 SQLite 任務表，保留其他必要數據

## 實現步驟

### Phase 1: Redis 連接初始化
- 添加 redis 依賴：`github.com/redis/go-redis/v9`
- 在 `masterNodeServer` 結構中添加 Redis 客戶端
- 從環境變量讀取 Redis 地址（默認：localhost:6379）
- 啟動時測試 Redis 連接

### Phase 2: 任務數據遷移
**Redis Key 設計**
- 單個任務：`task:{task_id}` (Hash)
- 任務索引：`tasks:owner:{owner}` (Set) - 存儲所有該用戶的任務ID
- 活躍任務：`tasks:active` (Set) - 存儲所有 PENDING/DISPATCHED/RUNNING 的任務ID
- 任務日誌：`task:{task_id}:logs` (List)

**Hash 字段**
```
task:{task_id} = {
  task_id: string
  owner: string
  worker_id: string
  worker_ip: string
  status: string (PENDING/DISPATCHED/RUNNING/COMPLETED/FAILED/STOPPED)
  status_message: string
  output: string (if available)
  result_torrent: string (if available)
  torrent_source: string
  expected_btih: string
  cpu_usage: number
  memory_usage: number
  gpu_usage: number
  gpu_memory_usage: number
  req_cpu_score: number
  req_gpu_score: number
  req_memory_gb: number
  req_gpu_memory_gb: number
  host_count: number
  billing_settled: 0/1
  billed_amount: number
  created_at: unix timestamp
  updated_at: unix timestamp
  last_update: unix timestamp (for timeout checking)
  last_settlement: unix timestamp
  retry_count: number
}
```

### Phase 3: 代碼變更
1. **替換 saveTaskLocked()**
   - 使用 Redis HSET 而不是 SQLite INSERT
   - 自動維護索引和活躍任務集

2. **替換 loadTasksFromDB()**
   - 啟動時只從 Redis 加載活躍任務（status != COMPLETED/FAILED/STOPPED）
   - 減少內存占用

3. **新增輔助函數**
   - `loadTaskFromRedis(ctx, taskID)` - 單個任務讀取
   - `getAllTasksForOwner(ctx, owner)` - 用戶任務查詢
   - `getActiveTaskIds(ctx)` - 活躍任務列表

4. **TTL 設置**
   - COMPLETED/FAILED/STOPPED 任務設置 TTL = 7 天
   - 自動清理過期任務

### Phase 4: 測試和驗證
- 單元測試：Redis 序列化/反序列化
- 集成測試：任務生命週期（dispatch → timeout → redispatch）
- 性能測試：任務查詢和更新延遲

## 遷移時間線
1. **立即**：添加 Redis 日誌記錄（已完成 ✓）
2. **本週**：實現 Phase 1-3
3. **測試周期**：Phase 4 驗證
4. **上線**：移除 SQLite 任務表，完全使用 Redis

## 備註
- 保持磁盤上的任務文件存儲（種子、輸出）
- Redis 只存儲元數據，便於橫向擴展
- 支持多個 nodepool 實例共享任務狀態

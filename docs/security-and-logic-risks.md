# HiveMind 安全風險與邏輯缺陷清單（以程式碼為準）

> 本文件列出目前從程式碼可直接觀察到的：
> - 可能的**漏洞**（安全性問題）
> - 高風險的**系統邏輯錯誤/一致性問題**
>
> 主要來源（舉例）：
> - `node_pool/master_node_service.py`
> - `node_pool/worker_node_service.py`
> - `node_pool/node_manager_service.py`
> - `node_pool/node_manager.py`
> - `node_pool/config.py`
> - `worker/src/hivemind_worker/task_executor.py`
> - `worker/src/hivemind_worker/grpc_servicer.py`
> - `master/hivemind_master/src/hivemind_master/master_node.py`

---

## 0. 重要前提（設計層級）

- 此專案的核心功能本質上是「分散式任務執行」，所以**任務 ZIP 內容通常是非信任輸入**。
- 若沒有隔離（container/VM + 最小權限 + 網路隔離），整體系統天然具備 **RCE（遠端執行任意程式）風險**。
- 本文件仍會指出「超出預期」的漏洞（例如 path traversal / zip slip / 未授權操作），以及明顯的邏輯錯誤。

---

## 1. Critical（可被濫用造成重大破壞/資料外洩）

## 1.1 NodePool 端 task_id 可能造成 Path Traversal（任意路徑寫入）

- **位置**：`node_pool/master_node_service.py` → `FileStorageManager.get_task_zip_path/get_result_zip_path` + `store_task_zip/store_result_zip`
- **現象**：
  - 檔名為 `os.path.join(base_path, f"task_{task_id}.zip")` / `result_{task_id}.zip`.
  - `task_id` 來源於上游 RPC（Master 上傳），未看到白名單字元限制或路徑正規化/拒絕 `../`.
- **影響**：
  - 惡意 `task_id`（例如含 `../`、絕對路徑片段）可能導致覆寫/建立非預期路徑檔案（依 OS/權限而定）.
- **建議**：
  - 對 `task_id` 做嚴格規範（例如 `^[a-zA-Z0-9_-]{1,64}$`）.
  - 寫檔前對產出路徑 `normpath` 後確認仍在 `TASK_STORAGE_PATH` 之下.

## 1.2 Worker 端 ZIP 解壓縮存在 Zip Slip（路徑穿越）風險

- **位置**：`worker/src/hivemind_worker/task_executor.py` → `ZipFile(...).extractall(workspace)`
- **現象**：
  - 直接 `extractall(workspace)`，未檢查 ZIP entry 是否包含 `../` 或絕對路徑.
- **影響**：
  - 任務 ZIP 若含惡意檔名，可把檔案解壓到 `workspace` 外，覆寫 worker 主機上的任意可寫檔案.
- **建議**：
  - 實作安全解壓縮：逐一檢查 `ZipInfo.filename` 正規化後必須仍落在 `workspace`.

## 1.3 多個 gRPC 服務缺少認證/授權（可偽造上報、污染狀態/結果）

- **位置**：
  - `node_pool/node_manager_service.py` → `RegisterWorkerNode`, `ReportStatus`
  - `node_pool/worker_node_service.py` → `TaskOutputUpload`, `TaskResultUpload`, `TaskUsage`
- **現象**：
  - 這些 RPC 目前看不到 JWT 驗證或 caller 身份校驗.
  - `TaskUsage(task_id="0")` 甚至用 `request.token` 當 `node_id`.
- **影響**：
  - 未授權 caller 可以：
    - 偽造節點註冊與狀態（污染排程/面板）
    - 偽造任務 output/result（污染任務結果、誘發錯誤的資源釋放）
    - 偽造節點 usage（影響監控/資源面板）
- **建議**：
  - 強制所有會改寫狀態的 RPC 驗證 JWT（metadata `authorization: Bearer <token>`）.
  - `TaskUsage` 增加明確欄位 `node_id`，不要用 `token` 欄位承載.

## 1.4 gRPC 使用 insecure channel（token/任務資料明文傳輸）

- **位置**：
  - Master：`master/hivemind_master/src/hivemind_master/master_node.py` → `grpc.insecure_channel(...)`
  - NodePool -> Worker stop：`node_pool/master_node_service.py` → `grpc.insecure_channel(f"{worker_host}:{worker_port}")`
- **影響**：
  - 若跨機器網路環境，JWT token / 任務 ZIP / 結果 ZIP 都可能被竊聽或竄改.
- **建議**：
  - 上線環境提供 TLS（mTLS 更佳），或把 gRPC 放在受控內網 + service mesh.

---

## 2. High（容易造成安全事故或核心流程錯誤）

## 2.1 `TaskUsage(task_id="0")` 濫用 `token` 欄位當 node_id（可被冒用）

- **位置**：`node_pool/worker_node_service.py: TaskUsage`
- **現象**：
  - 當 `task_id == "0"` 時，`node_id = (request.token or "").strip()`.
- **影響**：
  - 任意 caller 只要知道某節點 id（通常就是 username），就可上報該節點的 usage.
- **建議**：
  - 拆欄位：`node_id` 與 `auth_token`（metadata）分離.
  - server 端以 JWT 的 username 綁定 node_id.

## 2.2 任務所有權（user_id）語意不一致，導致授權判斷混亂

- **位置**：`node_pool/master_node_service.py`
- **現象**：
  - 上傳 `UploadTask` 時：`store_task(..., user_id=username)`（`task:<id>.user_id` 存的是 username）.
  - `GetAllTasks` 以 username 比對（且註解明示修正 user_id 問題）.
  - `StopTask` 允許 `task_user_id` 同時比對 user_id 或 username（相容處理）.
  - 但 `GetTaskResult` 版本間存在不一致（有的只比對 username）.
- **影響**：
  - 容易出現：
    - 合法使用者拿不到結果/不能操作
    - 或授權判斷出現漏洞（視具體路徑而定）
- **建議**：
  - 統一：Redis 內 `task:<id>.user_id` 永遠存 DB 的 user_id（整數字串），另存 `username` 供 display.

## 2.3 `master_node_service.py` 內可能存在「同名方法重複定義/覆蓋」風險

- **位置**：`node_pool/master_node_service.py` → `GetTaskResult`（在檔案中可見不只一段實作）
- **現象**：
  - Python class 若同名方法出現兩次，後者會覆蓋前者.
  - 讀到的其中一個版本存在「有 token 才驗證」的分支，容易形成意外的未授權路徑.
- **影響**：
  - 行為與開發者預期不一致，且難以維護與測試.
- **建議**：
  - 僅保留單一 `GetTaskResult`，並加上單元測試覆蓋：
    - token 無效
    - token 空
    - 非 owner
    - owner

## 2.4 NodePool 在回傳結果時「清理 Redis」的時序風險（可能造成資料遺失）

- **位置**：`node_pool/master_node_service.py` → 讀到的 `GetTaskResult` 版本：
  - `cleanup_task_data(task_id)`
  - `delayed_cleanup_task_files(task_id, delay_seconds=10)`
- **影響**：
  - client 若下載中斷/重試，Redis metadata/logs 可能已被清掉，導致無法重試下載或 debug 資訊遺失.
- **建議**：
  - 以「顯式 ACK」或「結果保留 TTL」策略替代立即清理.

## 2.5 `node_pool/config.py` 的 `JWT_SECRET_KEY` 可能觸發 `NameError`（邏輯錯）

- **位置**：`node_pool/config.py`
- **現象**：
  - `JWT_SECRET_KEY` block 中使用 `secrets.token_urlsafe(32)`.
  - 但 `secrets` 只在 `SECRET_KEY` fallback 的 if-block 中才被 `import`；若 `SECRET_KEY` 已設定，`secrets` 可能未定義.
- **影響**：
  - 某些環境下啟動直接 crash.
- **建議**：
  - 在檔案頂部或 class body 一律 `import secrets`.

---

## 3. Medium（易導致效能/穩定性問題，或成為攻擊面的放大器）

## 3.1 Worker ZIP 驗證只做 `testzip()`，未限制壓縮炸彈（Zip Bomb）

- **位置**：`worker/src/hivemind_worker/grpc_servicer.py: ExecuteTask`（`zip_ref.testzip()`）
- **現象**：
  - `testzip()` 驗證 CRC，不會阻止「高壓縮比、解壓後超大」的 payload.
- **影響**：
  - worker 磁碟耗盡 / OOM，造成拒絕服務.
- **建議**：
  - 解析 ZipInfo，限制總解壓大小、單檔大小、檔案數.

## 3.2 gRPC message size 極大（DoS 放大）

- **位置**：`master/hivemind_master/src/hivemind_master/master_node.py` 設定 `grpc.max_receive_message_length` / `grpc.max_send_message_length` 約 1GB.
- **影響**：
  - 更容易被大 payload 造成記憶體壓力.
- **建議**：
  - 改成 chunk/streaming 或 object storage + URL.

## 3.3 Redis `KEYS` 掃描（阻塞風險）

- **位置**：`node_pool/master_node_service.py: GetAllTasks`（`redis_client.keys("task:*")`）
- **影響**：
  - 任務量變大會阻塞 Redis.
- **建議**：
  - 使用 `SCAN` 或維護 `user:<id>:tasks` 索引作查詢.

## 3.4 NodePool Redis 連線設定被硬編碼忽略 Config（可用性/部署風險）

- **位置**：`node_pool/node_manager.py` → `redis.Redis(host='localhost', port=6379, db=0, ...)`
- **影響**：
  - `Config.REDIS_HOST/PORT/DB` 設了也不生效，部署容易踩坑.
- **建議**：
  - 統一從 `Config` 讀取.

## 3.5 計量單位不一致（CPU/MEM usage % vs MB）

- **位置**：
  - NodePool `TaskManager.update_task_usage` 把 usage clamp 到 0-100（暗示 percent）.
  - Worker `worker/src/hivemind_worker/grpc_servicer.py: ReportRunningStatus` 記錄 `memory_usage` 為 MB.
- **影響**：
  - 面板顯示/排程策略會錯.
- **建議**：
  - 明確定義 proto 欄位單位（建議改名 `*_percent` 或 `*_mb`）。

---

## 4. Low（建議修但不會立即爆炸）

## 4.1 Master 預設 `FLASK_SECRET_KEY` 為固定字串

- **位置**：`master/hivemind_master/src/hivemind_master/master_node.py` → `FLASK_SECRET_KEY = ... 'a-default-master-secret-key'`
- **影響**：
  - 若 Flask session/cookie 有使用，可能被偽造.
- **建議**：
  - 只在 dev 給預設值，上線強制環境變數.

## 4.2 Docker image 使用 `:latest` + 執行時安裝 requirements（供應鏈/可重現性）

- **位置**：`worker/src/hivemind_worker/task_executor.py` → `"justin308/hivemind-worker:latest"`，且執行時 `pip install -r requirements.txt`
- **影響**：
  - build 不可重現、容易被供應鏈污染.
- **建議**：
  - 固定 image digest 或 version tag；requirements pin 版本；或改成 build task image.

---

## 5. 建議優先修復順序（從最容易被打到最核心）

1. `task_id` 路徑安全（NodePool 寫檔 path traversal）
2. Worker safe unzip（Zip Slip + Zip Bomb）
3. RPC 身份驗證一致化（Register/ReportStatus/TaskUsage/TaskResultUpload 等）
4. GetTaskResult/所有權欄位一致化（避免授權漏洞與行為不一致）
5. TLS/mTLS（跨機器傳輸）

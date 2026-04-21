# Node Pool: Python → Go 對照表

下表將你提供的 Python `node_pool` 模組檔案對應到我在 `services/nodepool` 建立的 Go scaffold（僅結構對映，尚無實作）：

- `__init__.py` → `pkg/models/models.go`
  - 說明：套件初始化、共有常量與型別，對應到 Go 的共用型別檔案。

- `auth_interceptor.py` → `pkg/server/auth_interceptor.go` (建議新增)
  - 說明：gRPC 攔截器/權限驗證邏輯（token 驗證），建議在 `pkg/server` 下新增攔截器檔。

- `config.py` → `pkg/config/config.go`
  - 說明：環境變數與設定讀取；已建立 `pkg/config/config.go`。

- `database_manager.py` → `pkg/storage/storage.go` + DB 實作 (postgres)
  - 說明：資料存取的抽象層；對應到 `pkg/storage`，可把 DB 操作拆成 `db.go`、`migrations.go` 等。

- `database_migration.py` → `infra/migrations/` 或 `pkg/storage/migrations.go` (建議)
  - 說明：資料庫遷移腳本/管理工具，可放 infra 或 storage 下。

- `generate_proto.py` → 開發流程說明（`protoc` 生成）
  - 說明：Python 版用該腳本產生 pb，對應可用 `protoc --go_out` / `--go-grpc_out` 生成 Go stub；保留為 repository 文檔或 Makefile 一部分。

- `nodepool.proto` → `hivemind.proto`（已位於 repo root）
  - 說明：共用 proto 定義，應用於生成 Python / Go /其他語言。

- `nodepool_pb2.py`, `nodepool_pb2_grpc.py` → 由 `protoc` 生成的 Python stub
  - 對應：`pkg/gen` 或直接在 build 步驟使用 `protoc` 生成 Go stub（`pkg/gen` 可存放自動生成檔案）。

- `node_pool_server.py` → `pkg/server/server.go`
  - 說明：gRPC 伺服器主體；我已建立 `pkg/server/server.go` 作為啟動與註冊點。

- `node_manager.py` / `node_manager_service.py` → `pkg/handlers/register.go`、`pkg/handlers/report.go`
  - 說明：節點註冊/狀態回報邏輯會分拆成 handler 函式；目前 `pkg/handlers` 已建立相對應 stub。

- `worker_node_service.py` → 對應 `pkg/handlers/task.go` 與 `pkg/scheduler` + `services/worker`
  - 說明：Worker 端 RPC 的 server-side 與 nodepool 端的 handler（調度/下發）兩側都會有實作；在 nodepool 我放在 `pkg/handlers/task.go`（Upload/Execute/Stop 對應）。

- `master_node_service.py` → `services/master` 與 `pkg/handlers/task.go`
  - 說明：Master 與 nodepool 的 RPC 交互，nodepool 端 handler 在 `task.go` 處理上傳/查結果/取消。

- `user_service.py`, `user_manager.py` → `pkg/handlers/auth.go` (建議新增) + `services/master` 的業務層
  - 說明：使用者認證/token 管理應放在專責檔案（建議 `pkg/handlers/auth.go` 與 `pkg/storage` 的 user table）。

- `monitor_service.py` → `pkg/monitor/monitor.go` (建議新增)
  - 說明：系統/節點監控（metrics、health check），建議新增 `pkg/monitor` 包。

- `task_cleanup_scheduler.py`, `test_multi_task_distribution.py` → `pkg/scheduler/*` 和 `tests/`
  - 說明：排程、測試與清理工作，對應到 `pkg/scheduler` 及 repo 的測試目錄。

- `resource_manager` (目錄) → `pkg/scheduler` / `pkg/resources` (建議)
  - 說明：資源打分、GPU/CPU 計分，可放在 `pkg/resources` 或 `pkg/scheduler` 子模組。

- `templates/`, `templates/*` → `frontend/` 或 `templates/`（保留）
  - 說明：若有 web UI，模板放前端或獨立 template 目錄。

- `users.db` → `infra/` 或 `.dev` 資料（非程式碼）
  - 說明：示範用 SQLite DB，可移入 `infra/` 或 migration 管理下。


建議的映射檔案/新增檔名（Go side）
- `services/nodepool/pkg/server/server.go` (已建立)
- `services/nodepool/pkg/server/auth_interceptor.go` (建議)
- `services/nodepool/pkg/handlers/register.go` (已建立)
- `services/nodepool/pkg/handlers/report.go` (已建立)
- `services/nodepool/pkg/handlers/task.go` (已建立)
- `services/nodepool/pkg/handlers/output.go` (已建立)
- `services/nodepool/pkg/storage/storage.go` (已建立)
- `services/nodepool/pkg/config/config.go` (已建立)
- `services/nodepool/pkg/models/models.go` (已建立)
- `services/nodepool/pkg/scheduler/scheduler.go` (已建立)
- `services/nodepool/pkg/monitor/monitor.go` (建議新增)
- `services/nodepool/pkg/gen/` (放 `protoc` 產生的 Go pb 檔)
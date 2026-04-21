# Master: Python → Go 對照表

參考目錄（來自你的附件）: `templates_master`, `grpc_auth_client.py`, `master_node.py`, `nodepool_pb2_grpc.py`, `nodepool_pb2.py`, `grpc_services.py`, `vpn.py`, `CONFIG.example.yaml`, `README.md` 等。

下表為建議的檔案級對映（結構化、模組化）：

- `master_node.py` → `services/master/main.go` + `services/master/pkg/server/*`
  - 說明：程序啟動與 CLI/HTTP/gRPC 接口啟動點，Go 端使用 `main.go` 呼叫 `pkg/server.Start()`。

- `grpc_services.py` → `services/master/pkg/handlers/` (例如 `task.go`, `user.go`)
  - 說明：實作 UploadTask/GetTaskResult/GetAllUserTasks/StopTask/GetTasklog 等 RPC 的處理邏輯。

- `grpc_auth_client.py` → `services/master/pkg/auth/auth_client.go` 或 `pkg/auth/*`
  - 說明：與認證系統互動（token 取得/刷新/驗證），建議放 `pkg/auth` 包並提供中介/攔截器（interceptor）用於 gRPC 呼叫。

- `nodepool_pb2.py`, `nodepool_pb2_grpc.py` → 由 `hivemind.proto` 產生的 Go stub，放在 `services/master/pkg/gen`（自動生成檔）
  - 建議在 repo 加入 `Makefile` / `scripts/generate_protos.sh` 來產生 Go/Python 等語言的 pb 檔案。

- `templates_master/` → `frontend/templates_master/` 或 `services/master/templates/`
  - 說明：web UI / email templates 等；前端或 master 用於渲染回應的模板都可放在此目錄。

- `CONFIG.example.yaml` → `services/master/config/config.go`（或 `infra/config.example.yaml`）
  - 說明：設定樣板，Go 端讀取對應環境變數或 YAML 檔。

- `vpn.py`, `wg0.conf`, `wintun.dll`, `wireguardlib.dll` → `infra/vpn/` 或 `services/master/pkg/vpn/`（建議）
  - 說明：Tailscale/WireGuard 設定與外部執行工具、驅動檔，應以 infra 文件或啟動 script 管理，非核心業務程式。

- `README.md`、`pyproject.toml`、`test_master_auth.py` → `docs/` 與 `tests/` 對應
  - 說明：測試與說明檔統一管理。

建議的 Go 目錄（services/master） scaffold：

- `services/master/go.mod`
- `services/master/main.go` — 啟動點，呼叫 `pkg/server.Start()`
- `services/master/pkg/server/server.go` — gRPC server 啟動與服務註冊
- `services/master/pkg/handlers/task.go` — `UploadTask`, `GetTaskResult`, `GetAllUserTasks`, `StopTask`, `GetTasklog`
- `services/master/pkg/handlers/user.go` — 登入/權限相關 RPC wrapper
- `services/master/pkg/auth/auth.go` — token 驗證、RefreshToken wrapper（可含 gRPC 攔截器）
- `services/master/pkg/config/config.go` — 設定讀取
- `services/master/pkg/gen/` — 放 `protoc` 生成的 Go pb 代碼
- `services/master/templates/` — 如果需要 server-side template
- `services/master/docs/` — master 專屬文件（部署、vpn、config 範例）

可執行的下一步（你選一）：
- 1) 我建立上述 Go scaffold 檔案（僅檔頭與 TODO，不實作邏輯）。
- 2) 我只產生更詳細的函式級對照表（每個 Python 函式對應建議的 Go 函式簽名）。
- 3) 我幫你新增 `scripts/generate_protos.sh` 與 `services/master/pkg/gen` 的說明，方便後續 `protoc` 使用。

回覆數字 1、2 或 3 即可。
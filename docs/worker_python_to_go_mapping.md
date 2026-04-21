# Worker: Python → Go 對照表

參考你提供的 `hivemind_worker` 檔案列表，下面為建議的對映（檔案級）：

- `__init__.py` → `pkg/models/models.go`
- `_main_.py` → `main.go` (已存在)
- `auto_update.py` → `pkg/agent/autoupdate.go` (建議)
- `config.py` → `pkg/config/config.go` (已建立)
- `credential_store.py` → `pkg/credentials/credentials.go` (已建立)
- `docker_utils.py` → `pkg/executor/docker_utils.go` (建議)
- `grpc_auth_client.py` / `grpc_client.py` → `pkg/agent/grpc_client.go` (已建立)
- `grpc_servicer.py` → `pkg/server` (server handlers)
- `heartbeat.py` → `pkg/handlers/heartbeat.go` (已建立)
- `network_utils.py` → `pkg/agent/network_utils.go` (建議)
- `performance_calculator_v3.py` → `pkg/resource/resource_manager.go` (已建立)
- `psutil.*` → `pkg/monitor/system_metrics.go` (建議)
- `resource_manager.py` / `resource_monitor.py` → `pkg/resource/*` (已建立部分)
- `secure_store.py` / `session_manager.py` → `pkg/credentials` / `pkg/session` (建議)
- `system_metrics.py` → `pkg/monitor/system_metrics.go` (建議)
- `task_executor.py` → `pkg/executor/executor.go` (已建立)
- `webapp.py` → 可保留在 `frontend/` 或 `pkg/web` (建議)

已在 `services/worker/pkg` 建立的 scaffold：
- `pkg/server/server.go`
- `pkg/handlers/registration.go`
- `pkg/handlers/heartbeat.go`
- `pkg/executor/executor.go`
- `pkg/resource/resource_manager.go`
- `pkg/credentials/credentials.go`
- `pkg/agent/grpc_client.go`
- `pkg/config/config.go`
- `pkg/models/models.go`

下一步我可以：
- 1) 再產生細節級函式對照（Python 函式 → 建議的 Go 函式簽名）。
- 2) 建立額外建議檔案（`pkg/agent/network_utils.go`, `pkg/executor/docker_utils.go`, `pkg/monitor/system_metrics.go` 等）。

要我接著做 1 還是 2？
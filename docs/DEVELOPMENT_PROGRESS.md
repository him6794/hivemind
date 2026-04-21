# NodePool 開發進度

最後更新：2026-04-20

## 2026-04-20 進度更新（本輪）

### 已完成（調度/管理）

- 任務多路由停止（不做 subtask 拆分，沿用「上傳即已拆分任務」前提）
  - 同一 task 可追蹤多 worker route，`StopTask` 會嘗試停止所有 route，回傳完整/部分停止結果。
  - 檔案：[services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
- 任務查詢治理 API（HTTP）
  - `GET /api/tasks` 支援 `status`、`q`、`from_ts`、`to_ts`、`limit`、`offset`、`sort_by`、`order`。
  - 檔案：[services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
- 批次停止 API
  - `POST /api/stop-tasks`，一次停止多筆 task。
  - 檔案：[services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
- Worker 管理 API
  - `GET /api/workers`（已做）與 `POST /api/remove-worker`（新增）。
  - 檔案：[services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
- Master 代理層同步
  - `GET /api/tasks` 改為代理 nodepool HTTP（支援查詢參數透傳）。
  - 新增 `POST /api/stop-tasks`、`POST /api/remove-worker` 代理。
  - 檔案：[services/master/cmd/server/main.go](services/master/cmd/server/main.go)
- 前端管理面補齊
  - Master UI：任務篩選/搜尋/分頁、批次停止。
  - Worker UI：叢集 worker 移除操作。
  - 檔案：[frontend/master-ui/src/App.jsx](frontend/master-ui/src/App.jsx)、[frontend/worker-ui/src/App.jsx](frontend/worker-ui/src/App.jsx)

### 驗證（由 agent 實際啟動服務並跑流程）

- 啟動服務：nodepool、master、worker（auto-register）
- 實測流程：
  1. 登入（`/api/login`）
  2. 查詢 workers（`/api/workers`）
  3. 註冊 worker（`/api/register-worker`）
  4. 上傳任務（`/api/upload-task`，含 `host_count`）
  5. 查詢任務（`/api/tasks`，含篩選與分頁）
  6. 批次停止（`/api/stop-tasks`）
  7. 查詢轉帳彙總（`/api/transfers?aggregate=1`）
  8. 移除 worker（`/api/remove-worker`）
- Build/Test：
  - `services/nodepool`: `go test ./...` ✅
  - `services/master`: `go test ./...` ✅
  - `frontend/master-ui`: `npm run build` ✅
  - `frontend/worker-ui`: `npm run build` ✅

### 仍未完成（本輪刻意不做）

- 任務 subtask 拆分與聚合回收（你已指定先不做）。
- Worker 持久化（目前仍 in-memory repository）。
- RBAC/ACL 與審計日誌。
- Metrics/Tracing（Prometheus/OpenTelemetry）。

## 已完成

- 實作線程安全的 In-memory WorkerRepository
  - 檔案：[services/nodepool/internal/repository/worker_repository.go](services/nodepool/internal/repository/worker_repository.go)
  - 提供：SaveWorker、GetWorker、ListWorkers、DeleteWorker、UpdateHeartbeat
- 實作 WorkerService（service 層、business logic）
  - 檔案：[services/nodepool/internal/service/worker_service.go](services/nodepool/internal/service/worker_service.go)
  - 功能：RegisterWorker、Heartbeat、ListAvailableWorkers、RemoveWorker
  - Heartbeat 超時判定：30s（超時標記為 OFFLINE）
- 實作 handler 適配層（transport -> service）
  - 檔案：[services/nodepool/internal/handler/worker_handler.go](services/nodepool/internal/handler/worker_handler.go)
  - 條理化 Register/Heartbeat/List/Remove 介面，方便之後接上 gRPC
- 新增單元測試
  - repository 測試：[services/nodepool/internal/repository/worker_repository_test.go](services/nodepool/internal/repository/worker_repository_test.go)
  - service 測試：[services/nodepool/internal/service/worker_service_test.go](services/nodepool/internal/service/worker_service_test.go)
  - 測試結果：`go test ./...` 在 `services/nodepool` 模組下通過
- 初始 gRPC 支援（minimal 手寫 pb）與伺服器
  - 最小 pb 定義（手寫，便於本地運行）：[services/nodepool/pb/hivemind.pb.go](services/nodepool/pb/hivemind.pb.go)
  - gRPC server 實作（NodeManagerService 的 RegisterWorkerNode / ReportStatus）：[services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
  - 更新 `go.mod` 並加入 gRPC 依賴；成功編譯通過
- 臨時 HTTP 測試伺服器曾用於快速驗證（已替換為 gRPC）

## 測試與運行

- 執行所有測試（在 nodepool 目錄）：

```bash
cd services/nodepool
go test ./...
```

- 啟動 gRPC 伺服器（預設監聽 `:50051`）：

```bash
cd services/nodepool
go run ./cmd/server
```

- gRPC 服務端點（已實作）：
  - `nodepool.NodeManagerService.RegisterWorkerNode`
  - `nodepool.NodeManagerService.ReportStatus`

## 已知限制與備註

- 目前 `pb/hivemind.pb.go` 為手寫的最小 wrapper，尚未使用 protoc 產生的完整 Go 版 protobuf 定義。
- Handler 與 Service 皆使用內部 `repository.Worker` 型別；整合 protoc-generated types 後需做少量 mapping。
- 未實作權限驗證、日誌結構化、持久化（目前為 memory store）。

## 下一步建議（優先順序）

1. 使用 `protoc` 與 `protoc-gen-go` / `protoc-gen-go-grpc` 產生正式的 Go protobuf 程式，並替換手寫 `pb`。  
2. 為 gRPC 實作加入整合測試（可使用 `grpc-go` 客戶端或 `grpcurl` 做 smoke test）。  
3. 將 repository 抽象化介面導出，未來替換為 Redis/Postgres 等持久層。  
4. 加入認證 middleware（JWT）與結構化日誌（slog / zap）。  
5. 撰寫 CI workflow（執行 `go test`、`go vet`、`go build`）。

## 相關檔案清單（快速連結）

- [proto/hivemind.proto](proto/hivemind.proto)
- [services/nodepool/go.mod](services/nodepool/go.mod)
- [services/nodepool/internal/repository/worker_repository.go](services/nodepool/internal/repository/worker_repository.go)
- [services/nodepool/internal/service/worker_service.go](services/nodepool/internal/service/worker_service.go)
- [services/nodepool/internal/handler/worker_handler.go](services/nodepool/internal/handler/worker_handler.go)
- [services/nodepool/pb/hivemind.pb.go](services/nodepool/pb/hivemind.pb.go)
- [services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)

---

若要我現在執行第 1 步（用 protoc 產生正式 pb 並替換），或第 2 步（執行 smoke test），請回覆要執行的步驟編號。
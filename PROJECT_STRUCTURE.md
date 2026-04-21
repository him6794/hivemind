專案結構（已建立的檔案/目錄）

- `hivemind.proto`: gRPC 訊息與服務定義（已更新為 torrent-based flow）
- `readme.txt`: 原始說明檔

- services/
  - nodepool/
    - `go.mod`: Go module
    - `main.go`: 程式啟動，呼叫 `pkg/server.Start()`
    - `server.go`: 保留舊 stub（已移除主要邏輯）
    - `pkg/`:
      - `server/server.go`: gRPC server 啟動與註冊點
      - `handlers/`: 各 RPC handler（`register.go`, `report.go`, `task.go`, `output.go`）
      - `scheduler/scheduler.go`: 任務排程器 stub
      - `storage/storage.go`: 儲存層抽象（DB/Redis）
      - `models/models.go`: 非 proto 共享型別
      - `config/config.go`: 設定讀取
  - master/
    - `go.mod`
    - `main.go`: master service stub (上傳任務/查結果)
  - worker/
    - `go.mod`
    - `main.go`: worker service stub (註冊/回報/執行任務)

- executor-rs/
  - `Cargo.toml`: Rust crate config
  - src/
    - `lib.rs`: executor stub，負責 sandboxed Python 執行

- frontend/
  - `package.json`: frontend scaffold
  - src/
    - `App.jsx`: React stub

- infra/
  - `docker-compose.yml`: 範例服務部署（redis, postgres, nodepool, master, worker）

- docs/
  - `ARCHITECTURE.md`: 架構說明
- `PROJECT_STRUCTURE.md`: 此檔案（檔案用途與說明）

用途說明（簡短）
- Nodepool: 實作 gRPC server（使用 `hivemind.proto`），負責節點註冊、狀態回報、任務排程與分配。
- Master: 提供使用者上傳任務（torrent）與查詢任務狀態/結果的介面（客戶端 + server endpoints）。
- Worker: 註冊到 nodepool、下載 torrent、呼叫 executor 執行任務、上傳結果 torrent / 輸出。
- Executor: Rust 實作 sandbox，限制資源並執行 Python 任務。
- Frontend: 可選，提供任務提交與監控介面。

接下來建議
- 使用 `protoc` 生成 Go/其他語言的 stub，然後在各服務中實作 handler。
- 在 `infra/docker-compose.yml` 中加入 kafka / tailscale / 更多環境變數。
- 實作 Download + Torrent 管道或改為 object storage + signed URL。
# Hivemind 貢獻指南

架構、任務契約與部署設定見[使用與部署手冊](docs/HIVEMIND_MANUAL.zh-TW.md)，其他專題見[文檔索引](docs/README.md)。

## 1. 產品與信任邊界

修改前請閱讀 [AGENT.md](AGENT.md)、[AGENTS.md](AGENTS.md) 與所屬目錄的開發指引。

- Nodepool 是帳號、餘額、排程、任務狀態、Worker 身分與結算的唯一可信權威。
- Master 與 Worker 由使用者部署；它們回報的身分、資源、結果與費用都必須由 Nodepool 驗證。
- 官網提供產品入口與帳號中心，任務操作屬於 Master，Worker 註冊與本機資訊屬於 Worker。
- 瀏覽器不直接連 Nodepool；官網後端透過 `WEBSITE_NODEPOOL_GRPC_ADDR` 連線。
- 新 CPU managed 任務只接受 `hcode`，v0 僅供讀取歷史資料；GPU 與 general-compute 使用獨立契約。
- hcode 的 `max_cpt` 是包含平台費與所有副本的整個任務收費上限。
- 保留身分驗證、共識、用量驗證與隔離要求，不以單一 Worker 結算或較弱的執行方式繞過檢查。

## 2. 開發環境

### 後端

- `hivemind-rs/`：應用程式、API、Nodepool、資料庫、排程與 Worker。
- `executor-rs/`：封閉 DSL、GPU 與 general-compute runtime libraries。
- Hivemind CI 使用 Rust 1.98.1；workspace 未宣告 MSRV。工具鏈版本見 [CI](.github/workflows/ci.yml) 與對應 Dockerfile。
- 各平台需要 linker、protobuf compiler 與原生函式庫建置工具，安裝方式見手冊與 CI。
- `Makefile` 位於 repository root，`hivemind-rs/` 沒有獨立的 Makefile。

### 前端

三個前端各自安裝依賴、建置與測試：

| 目錄 | 用途 |
| --- | --- |
| `frontend/` | Next.js 官網與帳號中心 |
| `frontend/master-ui/` | Vite／React Master UI |
| `frontend/worker-ui/` | Vite／React Worker UI |

Next.js 官網需要 Node.js 20.9+，CI 使用 Node.js 20。一般建置使用 `npm ci`；更新依賴時使用 `npm install`，並一併檢查 lockfile。

### 設定與機密

以 [`.env.example`](.env.example) 為設定範本。`hivemind-bin` CLI 啟動服務時會搜尋工作目錄及父目錄的 `.env`；Windows 封裝的獨立 Master／Worker 入口只載入執行檔旁的 `.env`。設定再由環境變數或 `HIVEMIND_CONFIG` JSON 讀取。

`.env`、密碼、JWT、私鑰、Headscale API key 與本機 client state 不得提交。一般 Master／Worker client 不需要平台的 `JWT_SECRET`、資料庫密碼或 Nodepool signing private key。

## 3. 建置與測試

以下命令從 repository root 執行，適用於 PowerShell：

```powershell
cargo build --manifest-path hivemind-rs/Cargo.toml
cargo fmt --manifest-path hivemind-rs/Cargo.toml --all -- --check
cargo clippy --manifest-path hivemind-rs/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path hivemind-rs/Cargo.toml
```

修改 executor libraries 時，另執行：

```powershell
cargo fmt --manifest-path executor-rs/Cargo.toml --all -- --check
cargo clippy --manifest-path executor-rs/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path executor-rs/Cargo.toml
```

單一 crate 或測試：

```powershell
cargo test --manifest-path hivemind-rs/Cargo.toml -p hivemind-models
cargo test --manifest-path hivemind-rs/Cargo.toml "test_name_filter" -- --nocapture
```

將 `test_name_filter` 換成實際名稱，確認測試數量不為零。GPU、原生 WebView 等功能需要開啟對應 feature。

### 資料庫測試

將 `HIVEMIND_TEST_DATABASE_URL` 指向專用、可丟棄的 PostgreSQL。部分資料庫與結算測試在無法連線時會跳過或提早返回，執行後需確認案例數量與跳過項目。

**不要使用正式資料庫。** 測試可能清除或重建資料；測試記錄應包含資料庫連線與 schema 初始化結果。

### 前端

```powershell
npm --prefix frontend ci
npm --prefix frontend test
npm --prefix frontend run build

npm --prefix frontend/master-ui ci
npm --prefix frontend/master-ui test
npm --prefix frontend/master-ui run build

npm --prefix frontend/worker-ui ci
npm --prefix frontend/worker-ui test
npm --prefix frontend/worker-ui run build
```

服務連線與瀏覽器流程見 [Getting Started](docs/GETTING_STARTED.md)，原生視窗測試見 [SHADCN_UI.md](docs/SHADCN_UI.md)。

### 文檔

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-docs.Tests.ps1
```

此腳本直接執行，不需要 Pester。新增或修改文檔時，另檢查相對連結、範例語法、工作目錄與設定名稱。

## 4. 驗證範圍

| 修改範圍 | 測試重點 |
| --- | --- |
| DSL parser／evaluator／metering | 語法、語義、overflow、budget、value/output bounds |
| 排程與共識 | 不同副本、request/attempt binding、過期回應、no quorum、cancel fencing |
| 計費與結算 | 使用 PostgreSQL 驗證交易、冪等性、hold/refund 與有效用量 |
| API／auth | 未登入、錯誤 owner、過期 token、權限、正常請求與錯誤映射 |
| 前端 | 所屬介面的契約測試與建置，必要時追加瀏覽器、鍵盤與小螢幕測試 |
| Linux OCI／Windows HCS | provider/policy 測試，以及對應平台的實機執行測試 |
| Client 更新 | 簽章、digest、版本限制、activation/recovery；正式通道需在乾淨主機驗證 |
| 文檔 | 實作一致性、相對連結、範例與文檔契約 |

新功能需涵蓋核心行為，bug fix 需加入能重現問題的回歸測試。Mock 與 preflight 測試不能替代 provider 實際執行。

## 5. 程式碼與文檔風格

- 遵循所屬 crate／前端的命名、錯誤處理與註解習慣，Rust 使用 `cargo fmt` 與 Clippy。
- 三個前端的登入狀態與部署設定分開處理。
- 沿用既有 domain type 與版本化協定；計費與結算權限留在 Nodepool。
- 協定、語義或 cost model 的不相容變更需有版本、fixture、migration 與 rollout，不修改 frozen v0 的契約。
- 文檔寫明適用版本；操作命令附工作目錄、平台、前置需求與破壞性影響。
- Issue、一般日誌與截圖不得包含任務原始碼、input、JWT、私鑰或 callback secret。

## 6. 回報問題

請附上：

1. Commit／release 版本與 runtime ID。
2. 作業系統、架構、Rust／Node 版本與 feature flags。
3. Master／Nodepool／Worker 拓撲與 VPN 設定。
4. 最小重現步驟、預期結果與實際結果。
5. 去除機密後的錯誤碼、時間、task/attempt/round ID 與相關日誌。
6. 資料庫與 provider 環境，以及測試的執行、失敗或跳過結果。

安全問題請透過維護者指定的私人管道通報，不要在公開 issue 張貼正式環境的機密。

## 7. Commit 與 review

每個 feature 使用獨立、可 review／revert 的 commit。執行對應測試後再提交，不混入其他功能或無關格式變更。

使用 Conventional Commits，例如：

```text
feat(scheduler): add bounded admission policy
fix(runtime): reject overflowing integer arithmetic
test(database): cover idempotent usage settlement
docs(manual): document runtime configuration
```

提交前確認：

- [ ] 變更符合產品與信任邊界。
- [ ] 已執行對應測試，記錄失敗與跳過原因。
- [ ] 格式與 lint 檢查通過。
- [ ] 文檔、設定範例、proto、fixtures 與 migration 一致。
- [ ] Diff 沒有機密、本機 state 或無關 lockfile 變更。

PR 描述包含修改原因、契約影響、驗證結果、未測項目與 rollback 方式。

## 8. 授權

專案採用 MIT [LICENSE](LICENSE)。第三方依賴、vendored 元件與打包映像依各自授權；重新散布時需保留相應的授權文件與 notices。

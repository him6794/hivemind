# Hivemind 使用與部署手冊

> 文檔版本：2026-10-05（程式碼基準 `10d3c92e67034cfda345fb889029069dd40e0b4a`）
>
> 讀者：任務使用者、Worker 提供者、平台部署者與開發者。
>
> 用途：說明 Hivemind 任務、服務、設定、API、部署與維運方式。

## 目錄

1. [定位與產品狀態](#scope)
2. [服務角色與信任邊界](#architecture)
3. [程式碼結構與請求流程](#code-and-flow)
4. [環境、建置與本機啟動](#build)
5. [設定參考與連接埠](#configuration)
6. [帳號、登入與 VPN enrollment](#enrollment)
7. [Managed-function-v1 任務與 DSL](#managed)
8. [計量、共識、CPT 與結算](#settlement)
9. [HTTP／gRPC 介面與操作範例](#api)
10. [GPU、general compute 與 ONNX](#other-runtimes)
11. [正式部署、資料與維運](#operations)
12. [測試、release gates 與證據](#testing)
13. [故障排查](#troubleshooting)
14. [限制、待驗證項目與授權](#limits)
15. [原始碼與延伸文檔索引](#sources)

<a id="scope"></a>

## 1. 定位與產品狀態

### 1.1 Hivemind 是什麼

Hivemind 是以 Rust 為核心的分散式批次運算 runtime。使用者透過自架 Master 提交任務；Nodepool 驗證帳號、輸入、預算與 Worker，協調執行並保存結果及帳務；Worker 在自己的主機執行獲派工作。

新 managed 任務使用 **`managed-function-v1`**：受限制的 source function／JSON DSL，不執行任意主機程式。一般運算、GPU 與 ONNX 使用不同 runtime 契約與部署條件。

### 1.2 產品狀態

| 項目 | 當前狀態 |
| --- | --- |
| Managed DSL v1 | 封閉 interpreter、計量、執行限制與多 Worker consensus；接受新任務 |
| Managed DSL v0 | 僅供歷史資料相容讀取；拒絕新提交，未完成舊工作於啟動時退役 |
| Nodepool consensus | 預設 3 個不同 Worker、2 個一致結果；成功 output 需 certificate，不是獨立正確性證明或硬體 usage attestation |
| CPT 帳務 | Nodepool 管理 hold、有效 usage、fee、payment／refund；CPT 是內部單位，沒有兌換或外部支付網路 |
| 官網 | 產品入口、註冊、登入與帳號餘額；不提供任務操作或 Worker 控制 |
| Master UI | 任務提交、查詢、logs、observability、下載與歷史；沒有 API key 管理或任意 ZIP executable 提交 |
| Worker UI | 登入、註冊、本機資訊、dashboard 與收益顯示；控制操作依現有 API 範圍 |
| VPN enrollment | Rust Website API 與 client 提供一次性 enrollment／readiness 流程 |
| Linux／Windows general compute | 使用不同 provider 與 policy；alpha admission gates 及 fail-closed 路徑 |
| Signed client updates | 有驗簽、package 檢查、activation／rollback wiring |
| macOS／Android 等平台 | 有產品目標與設計文件；並非每個平台都有正式下載套件 |

<a id="architecture"></a>

## 2. 服務角色與信任邊界

### 2.1 官方服務與使用者服務

| 角色 | 部署者 | 核心責任 |
| --- | --- | --- |
| Official Site | 官方 | 公開產品內容、文件、帳號中心；Next.js 前端與 account BFF |
| Rust Website API | 官方 | 帳號 API、transfer 與受保護的 VPN enrollment 等 server-side API |
| Nodepool | 官方 | 帳號身分、balance、任務狀態、Worker 註冊、admission、排程、consensus、usage 與 settlement |
| Headscale | 官方控制面 | Overlay enrollment、peer identity 與 routing；不是帳務權威 |
| PostgreSQL／Redis | 官方控制面 | Nodepool 的持久化與協調資料；不供瀏覽器或 Worker 直接使用 |
| Master API／UI | 使用者 | 本機任務入口與操作介面；把請求送給 Nodepool |
| Worker runtime／UI | 使用者 | 本機執行、註冊、狀態與結果回報；不擁有結算權限 |

`hivemind-bin` 可在同一 process 組合多個角色；Nodepool 仍是可信權威，Master 與 Worker 仍是使用者部署的不可信 client。

### 2.2 Nodepool 與信任邊界

Nodepool 是帳號、餘額、任務、Worker 註冊、排程、結算的唯一權威。Master 與 Worker 由使用者部署，屬於不受信任的 client；Nodepool 以自己簽發的 token 驗證 caller，並從 server-side 記錄核對 owner、assigned Worker、round／replica／attempt、request digest 與 policy。Worker 回報的結果及 usage 是 claim，驗證後才可用於發布結果或結算。

Worker 本機 HTTP API 僅供本機控制，不作為 Nodepool 的容量或結果依據。Browser 不直接連 Nodepool；官網 BFF 在 server-side 使用 `WEBSITE_NODEPOOL_GRPC_ADDR`。目前該 gRPC client 使用 insecure transport，需由 operator 透過受控 network／overlay／proxy 保護。Quorum、VPN 或 provider 不可用時，不得改以單一 Worker、direct host execution 或 receipt-only 自行結算。

### 2.3 官網的產品邊界

官網提供產品資訊、帳號、餘額、安裝與文件入口；任務提交、進度、取消、結果／log 與 Worker 資源操作由 Master UI 和 Worker UI 負責。

**Next BFF 與 Rust Website API 是不同服務。** Next BFF 的 account routes 直接 server-side 呼叫 Nodepool UserService，不會把所有請求轉送到 Rust Website API，也沒有 `/api/vpn/config` route。

<a id="code-and-flow"></a>

## 3. 程式碼結構與請求流程

### 3.1 Repository 結構

| 路徑 | 用途 |
| --- | --- |
| `hivemind-rs/` | Backend workspace 與 service binary |
| `executor-rs/` | Managed interpreter／GPU／general-compute 等 runtime libraries／contracts |
| `proto/` | gRPC protobuf definitions |
| `frontend/` | Next.js Official Site／account center |
| `frontend/master-ui/` | Vite／React 18 Master UI |
| `frontend/worker-ui/` | Vite／React 18 Worker UI |
| `scripts/` | Build、release、docs、browser／native／provider checks |
| `templates/managed-function-v1/` | Active DSL 範例與 JSON input |
| `deploy/`、`infra/` | 平台／backend 部署資產與範例；不是所有檔案都代表現行正式拓撲 |
| `docs/` | 手冊、契約、驗證指南、計畫與歷史分析 |

`hivemind-rs/crates/` 的主要分工：

| Crate 目錄 | 責任 |
| --- | --- |
| `hivemind-bin` | CLI、角色啟動、service wiring 與 client lifecycle |
| `config` | Environment／JSON 設定與 validation |
| `proto` | Build 時產生 gRPC types |
| `models` | 共用 domain types 與 runtime identities |
| `database` | PostgreSQL repositories、migration、ledger／consensus records 與 Redis access |
| `auth` | Account login／registration、JWT、password hashing |
| `node-manager` | Nodepool gRPC、Worker 註冊、heartbeat、identity／readiness 與 task admission |
| `task-scheduler` | Dispatch、replica selection、round／timeout 與 stop／fencing |
| `managed-consensus` | Replicated policy、observation／canonical result 與 certificate validation |
| `client-runtime` | Shared client enrollment／overlay／update 與 platform adapters |
| `master-api` | HTTP task gateway、owner-scoped 查詢及 admin surfaces |
| `worker-executor` | Interpreter／backend 執行、Worker gRPC、本機 control HTTP |
| `vpn-service` | Headscale／overlay 與 enrollment 相關功能 |
| `website-api` | 官方 Rust account／VPN API |

Crate 目錄名不一定等於 Cargo package 名；使用 `cargo test -p` 前應核對該 `Cargo.toml`。

### 3.2 Active managed 任務的生命週期

1. 使用者登入 Master，從 UI 或 HTTP API 提交 v1 source、JSON input、resource requirements 與 task-wide budget。
2. Master 做 HTTP-level validation，帶著 bearer token 呼叫 Nodepool。
3. Nodepool 驗證 owner、runtime／semantics、source／input bounds、資源、共識 policy 與 balance／budget。
4. Enforce 模式建立需要的 usage hold，以及 durable round、replica assignment 和 Worker-slot reservation。
5. Scheduler 選出符合條件的 distinct Worker；每個 attempt 有自己的 execution token 與 request binding。
6. Worker 在封閉 interpreter 執行，回傳 typed result／failure 與 bounded receipt。
7. Nodepool 驗證並持久化 observation，對 canonical result 分組，達 strict majority 才建立 certificate 與成功 output。
8. Nodepool 另外驗證可計費 usage、計算 fee、settle／refund；成功 output 與 payment eligibility 不互相取代。
9. 未完成 replicas 必須 stop／fence；無法確認停止時保留 durable pending state，不能立刻重用 capacity。
10. 使用者透過 Master 取得狀態、log、observability 與可用 artifacts；不是每個 managed 結果都有舊 torrent download。

狀態不能只簡化成 `PENDING → COMPLETED`。觀察、dispatch、failure、cancellation、no-quorum、stop-pending 與 settlement 都可能有獨立狀態；以實際回應和持久化資料為準。

<a id="build"></a>

## 4. 環境、建置與本機啟動

### 4.1 先選擇你的情境

- **一般任務使用者**：使用 Master client／UI；不需要部署平台資料庫或取得 signing private key。
- **一般 Worker 使用者**：使用相符平台的 client，登入並等待 enrollment／registration／readiness；closed DSL 不要求安裝 Docker、WSL 或 HCS。
- **開發者**：建立 Rust／Node 環境，跑適用測試；需要 settlement evidence 時附加真正的 test PostgreSQL。
- **Operator**：部署官方 Nodepool、Website API、Headscale 與資料服務，準備 secrets、keys、routing、backend policy 和 release evidence。

取得套件時核對平台、版本、簽章／digest 與發布來源。

### 4.2 工具版本

- Hivemind CI 使用 **Rust 1.98.1**；Docker builder 固定使用另一版本 `rust:1.96-bookworm`。
- `executor-rs` 使用 edition 2024，`rust-version = 1.90`；Hivemind workspace 未宣告 MSRV。「Rust 1.70+」不適用。
- 官網 Next.js 16 依賴要求 **Node.js 20.9+**，CI 使用 Node.js 20；另外兩個 Vite frontend 的 Node.js 門檻較低。
- Compose 需要 Docker Engine／Desktop 與 `docker compose`。
- Release PowerShell scripts 主要供 Windows 使用；部分腳本直接呼叫 `npm.cmd`／`powershell`。
- Build 需要所選 target 的 compiler／linker、protobuf 與 native dependencies；GPU、WebView、general-compute 各有額外需求。

Linux Hivemind CI 安裝 `protobuf-compiler`。Executor rootless OCI tests 需要 `runc`、可讀取 `/proc/self/status` 及有效 subordinate UID／GID mapping；這些是 OCI provider／測試需求，不是 closed DSL 的依賴。

Windows CI 使用 Go 1.25.5、checksum-verified LLVM-MinGW 20260616 UCRT、libtailscale build assets、protoc 30.x，以及 MSVC／Visual Studio Build Tools。Go 用於 native connectivity build。Windows WebView2／native UI 與 ARM64 cross build 另有工具需求；一般已打包 client 不需自行重建工具鏈。

CI Hivemind 1.98.1、executor 1.90.0 與 Docker builder 1.96 是不同的建置路線；發布記錄應註明實際使用的 toolchain 與 features。

### 4.3 後端建置

以下命令從 repository root 執行：

```powershell
cargo build --manifest-path hivemind-rs/Cargo.toml
cargo build --release --manifest-path hivemind-rs/Cargo.toml
cargo test --manifest-path hivemind-rs/Cargo.toml
```

也可先進入 `hivemind-rs/` 再執行 `cargo build`／`cargo test`。`Makefile` 位於 repository root，`make build`／`make test` 也從 root 執行。Cargo 的 target 路徑可被設定覆寫，啟動 binary 時可用 `cargo run --manifest-path ...`。

### 4.4 三個前端分別建置

```powershell
npm --prefix frontend ci
npm --prefix frontend run build

npm --prefix frontend/master-ui ci
npm --prefix frontend/master-ui run build

npm --prefix frontend/worker-ui ci
npm --prefix frontend/worker-ui run build
```

每個 frontend 都有自己的 package／lockfile。`npm ci` 適合重現既有 dependency graph；有意更新 dependency 時才用 `npm install` 並 review lockfile。

Windows aggregate release build：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release-frontends.ps1
```

開發用 `npm run dev` 的 port 3000／3001、Compose host mapping，以及 native bundled UI 是不同 serving 情境；不要同時開兩個佔用相同 port 的 dev server。官網和 Master 的預設 dev port 都可能是 3000。

### 4.5 Service roles

當前 `hivemind-bin` 有 `master`、`nodepool`、`worker`、`website` 與 `all` 角色；`all` 在同一 process 啟動 Master、Rust Website API、Nodepool 和 Worker。它不會建立三台已登入的 eligible Worker，也不會配置正式 external topology。

在 role-specific 設定已備妥後，從 root 啟動例如：

```powershell
cargo run --release --manifest-path hivemind-rs/Cargo.toml --bin hivemind-bin -- nodepool
```

其他 roles 同理。`--help` 可查看目前 CLI；repository 另有 `submit`／`status`／`result` helpers，但其欄位與歷史 result semantics 仍需對照當前實作，不能假定 CLI 是所有 UI 功能的等價替代。

### 4.6 不要把 `make dev` 當成免設定啟動

目前 root target 會先 release build，使用舊 `docker-compose` CLI 啟動 Redis／PostgreSQL、固定等待，再 `cargo run ... all`。它：

- 不生成安全 `JWT_SECRET` 或 matching Ed25519 execution keys。
- 不等待完整 database readiness。
- 不建置／啟動三個 frontend。
- 不自動登入或建立足夠的 eligible Workers。
- 可能因 raw Compose 全檔 mandatory variables 解析而在「只啟動資料服務」時也失敗。

### 4.7 Local release smoke 與資料風險

僅在可丟棄的 Windows 開發環境執行；先確認 Compose project 與 volume 的所有權：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-stack-smoke.ps1 -KeepRunning
```

腳本可產生缺少的測試 secrets／execution key pair、分配未設定的 host ports、建立 closed-DSL smoke 用的空 backend registry，並檢查 service、HTML 與 health endpoints。

- 腳本未強制使用獨立 Compose project；一般清理會執行 `docker compose down -v`。
- 使用者指定的 volume 與預設 Worker state volume 可能含有既有資料；不要在現有 deployment 上執行。
- 未指定的 host port 會動態分配；明確設定的 port 必須可用。腳本結束會還原 process environment，其他 shell 不會自動取得 ports／secrets。
- `-KeepRunning` 仍會 build／start services；`-CheckOnly` 只做 packaging／config preflight。
- Root Compose 只有一個 Worker，沒有獨立 Rust Website API service。

執行瀏覽器 QA 前，將 `HIVEMIND_SITE_URL`、`HIVEMIND_MASTER_UI_URL`、`HIVEMIND_WORKER_UI_URL` 設為腳本輸出的 URL，再於 `frontend/` 執行 `npm run test:e2e`。Managed task success 需有足量 eligible Workers。

停止自己建立的 stack 可用 `docker compose down`；Compose 仍須有足以解析設定的 environment。`docker compose down -v` 會刪除 volumes；不要用它或 `make db-reset` 清理不明環境。

<a id="configuration"></a>

## 5. 設定參考與連接埠

### 5.1 設定來源

`HivemindConfig` 從 `HIVEMIND_CONFIG` 指定的 JSON file，或 process environment 讀取設定。服務啟動前依入口載入 `.env`：

- `hivemind-bin` CLI：搜尋目前工作目錄與父目錄。
- Windows 封裝的獨立 Master／Worker 入口：只載入執行檔旁的 `.env`。
- 其他獨立服務入口：搜尋目前工作目錄與父目錄。

請依啟動入口檢查實際載入位置，尤其是從不同目錄啟動 client 時。

`.env.example` 是 operator 設定範例，不是 live credentials，也不是普通 Master／Worker 的必要設定清單。Docker Compose 的 environment interpolation 與 Rust config loading 是不同機制。不要假定 JSON file 和環境變數一定逐欄位合併；以 loader 的選擇行為為準。

### 5.2 哪個角色需要哪些機密

| 設定／資產 | Nodepool | Rust Website API | 普通 Master／Worker |
| --- | --- | --- | --- |
| Database／Redis credentials | 需要 | 透過 Nodepool account API，不因此成為資料庫權威 | 不應取得 |
| 平台 `JWT_SECRET` | 需要 | 需要適用的 server-side auth 設定 | 不應要求一般下載使用者填寫 |
| Execution signing private key | Nodepool 專用 | 不因此取得 settlement authority | 不得提供 |
| Matching execution public key | 發布／provision verification identity | 視 service wiring | Worker 驗證 assignment 所需 |
| `HEADSCALE_API_KEY` | 控制面依 enrollment wiring | 受保護的 server-side enrollment 使用 | 不得放 client package／browser storage |
| 使用者 bearer JWT | Token authority 驗證 | Account／enrollment auth | 登入後在適用 session 邊界使用 |
| 一次性 VPN material | Server-side 發出／管理 | 受保護 route 提供 | 本機 process consume；不當成可重複 credentials |

JWT secret 不可使用 `CHANGE_ME_IN_PRODUCTION`，service validation 也要求足夠長度。Self-host execution key pair 必須匹配；bundled public default 不是你自己部署的 private key 的替代品。

### 5.3 地址不是同一件事

- **Listen address**：本機 bind，例如 `0.0.0.0:50053`。
- **Connect endpoint**：另一個 process 能連線的 target。
- **Advertise／callback address**：Worker 註冊給 Nodepool、讓 scheduler 實際連回來的地址。
- **Compose service hostname**：容器內的 `nodepool:50051`。
- **Host mapping**：瀏覽器或 host process 使用的 localhost port。

`0.0.0.0` 是 listen wildcard，不是可用的 callback target。容器裡的 `localhost` 是容器本身，不是 Nodepool service。Overlay／VPN routing 能否連通，也不能由 address string 正確直接推出。

### 5.4 常見連接埠

| 介面 | Service／本機預設或 raw Compose mapping | 說明 |
| --- | --- | --- |
| Nodepool gRPC | 50051 | 可信控制面；不是 browser API |
| Worker gRPC | 50053 | Task callback／execution endpoint；需可被 Nodepool 路由 |
| Master HTTP | 8082 | Master task gateway |
| Worker control HTTP | `127.0.0.1:18080` | Standalone 綁定 loopback；Compose host mapping 需限制網路存取 |
| Rust Website API | 8090 | 獨立 account／VPN API role |
| Official Site | raw Compose host 8080 | Next.js／BFF；dev port 另算 |
| Master UI | raw Compose host 3000 | Vite build 經 serving／native package 提供 |
| Worker UI | raw Compose host 3001 | Vite build 經 serving／native package 提供 |
| PostgreSQL | 5432 | 不應對任意 public callers 暴露 |
| Redis | 6379 | 同上 |

這些不是 live 網路資訊。Smoke 可以改 host ports，operator 也可以覆寫；以實際 role config 與打印 URL 為準。

### 5.5 設定清單

這張表選取影響啟動、trust、consensus 與 operator routing 的設定；不是所有 environment variables 的完整清單。更多欄位以 [config loader](../hivemind-rs/crates/config/src/lib.rs#L662) 與 [`.env.example`](../.env.example) 為準。

| 名稱 | 預設／用途 | 適用注意 |
| --- | --- | --- |
| `HIVEMIND_CONFIG` | 指向 JSON config | Loader 改走 file route；不要假定與 env 逐欄位合併 |
| `DATABASE_URL` | PostgreSQL connection string | Nodepool；正式 deployment 需覆寫範例帳密 |
| `REDIS_URL` | Redis connection string | Nodepool；不可發給下載 client |
| `JWT_SECRET` | 平台 account token signing secret | Nodepool／Website／all；trim 後至少 32 bytes，不能 default／empty |
| `WORKER_EXECUTION_PRIVATE_KEY_PEM` | Nodepool Ed25519 signing key；default empty | Nodepool／all 必須提供可解析私鑰 |
| `WORKER_EXECUTION_PUBLIC_KEY_PEM` | Worker Ed25519 verification key | Self-host 要與 Nodepool matching；可接受 literal `\n` normalization |
| `NODEPOOL_GRPC_ADDR` | `0.0.0.0:50051` | Listen／舊 connect context；不要拿 wildcard 當 remote target |
| `NODEPOOL_GRPC_ENDPOINT` | 明確 connect target override | 與 listen address 分開；使用適當 host／overlay endpoint |
| `MASTER_HTTP_ADDR` | `0.0.0.0:8082` | 使用者 Master；對外暴露前審查 local／network trust |
| `WEBSITE_HTTP_ADDR` | `0.0.0.0:8090` config field | Rust Website API role，非 Next serving port |
| `WEBSITE_NODEPOOL_GRPC_ADDR` | Next BFF server-side target | Compose 中應指 `nodepool:50051`；不要使用 browser public env |
| `WORKER_GRPC_ADDR` | `0.0.0.0:50053` | Worker execution listener |
| `WORKER_ADVERTISE_ADDR` | 註冊的 callback target | 必須可由 Nodepool route；不能是 unspecified bind address |
| `WORKER_CONTROL_HTTP_ADDR` | `127.0.0.1:18080` | Standalone local control；不應隨意公開 |
| `WEBSITE_API_BASE` | 本機 client 的 enrollment origin | 真正 Rust Website API；role-specific override 可以優先生效 |
| `MASTER_WEBSITE_API_BASE`／`WORKER_WEBSITE_API_BASE` | 角色專用 enrollment origin | 必須有 login 與 protected VPN config routes |
| `MASTER_VPN_AUTHKEY`／`WORKER_VPN_AUTHKEY` | Optional unattended startup | Operator credentials；不存普通 browser session |
| `HEADSCALE_LOGIN_SERVER` | Headscale control origin | 自動加入需搭配 Headscale auth key，不使用 client identity enrollment credential |
| `VPN_STARTUP_TIMEOUT_SECS` | Default 120；可設定 1–300 秒 | Bounded Nodepool readiness deadline |
| `WORKER_NODEPOOL_TOKEN` | Operator／已登入 registration flow 使用 | UI-driven registration 可初始為空；不是 public package 內嵌 token |
| `HIVEMIND_WORKER_STATE_ROOT` | Durable Worker state directory override | Absolute persistent path；stop-fence state 不應當 temp 刪除 |
| `EXECUTOR_SANDBOX_DIR` | `./sandbox` | Sandbox working root，不是安全隔離本身 |
| `EXECUTOR_SANDBOX_MODE` | Normal env-loading route 預設 `production` | `HivemindConfig::default()` 為 `dev`；環境 loader 預設 `production`，依載入路徑判定 |
| `MANAGED_CONSENSUS_ROLLOUT_MODE` | `enforce` | `observe` non-settling；`disabled` 不允許 downgrade |
| `MANAGED_CONSENSUS_REPLICA_COUNT` | 3 | 有界 replica policy；不是 `host_count` |
| `MANAGED_CONSENSUS_QUORUM` | 2 | Strict majority；不能因 Worker 不足而降低 |
| `MANAGED_CONSENSUS_MAX_REPLICAS` | 7 | Policy validation 上界 |
| `MANAGED_CONSENSUS_TIMEOUT_SECS` | 120 | Round deadline，與 Worker runtime timeout 不是同一控制 |
| `MANAGED_CONSENSUS_MAX_RESULT_BYTES` | 262144 | Configured result bound 仍受 typed protocol cap |
| `HIVEMIND_ADMIN_USERS` | Unset／comma-separated usernames | Master admin routes 另需 bearer 與 server permission |
| `HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE` | 60；≤0 封鎖提交 | Master process-local per-owner 60 秒 window，不是 unlimited switch |
| `LOG_LEVEL` | `info` | 不因 debug 就可以記錄 raw secrets |
| `VITE_API_BASE` | Master frontend build-time API base | Native package 可為 `/` same-origin；不是固定 remote Nodepool URL |
| `VITE_WORKER_CONTROL_BASE` | Worker frontend build-time control base | Native package可為 `/` same-origin；dev fallback 與 release wiring不同 |
| `HIVEMIND_TEST_DATABASE_URL` | Test PostgreSQL override | 只能指專用可丟棄 test DB |

Standalone Worker state 在 Windows 預設 `%LOCALAPPDATA%\Hivemind\Worker`；非 Windows 使用 `$XDG_STATE_HOME/hivemind/worker`，或 `$HOME/.local/state/hivemind/worker`。Compose 有自己的 persistent mount／named volume。Override 要保留安全權限與 durable ownership，不能把停機 fencing state 當普通 cache。

Smoke 可管理的 host-port variables 包括 `POSTGRES_HOST_PORT`、`REDIS_HOST_PORT`、`NODEPOOL_GRPC_HOST_PORT`、`TORRENT_TRACKER_HOST_PORT`、`TORRENT_SEED_HOST_PORT`、`MASTER_HTTP_HOST_PORT`、`WORKER_GRPC_HOST_PORT`、`WORKER_CONTROL_HOST_PORT`、`MASTER_UI_HOST_PORT`、`WORKER_UI_HOST_PORT`、`SITE_HOST_PORT`。Tracker／seed ports 用於相容與基礎設施；active managed input 使用 JSON data。

Worker loader／provider 的其他 registry variables 應依對應 backend 文檔 provision；單靠通用 sandbox env 不會自動建立 operator-owned OCI／HCS assets。

<a id="enrollment"></a>

## 6. 帳號、登入與 VPN enrollment

### 6.1 三個登入位置的差異

- Official Site：browser 呼叫 same-origin Next BFF account routes。
- Master：本機 Master UI／HTTP API 使用 account login，之後才執行自己的 task operations。
- Worker：本機 Worker UI／control API 登入，再完成 enrollment／registration。

帳號是平台身分；Master／Worker 的本機服務不是另一個獨立帳號權威。當前 Master `/api/register` 回傳 410，應到官網的 account flow 建立帳號。

### 6.2 Authenticated enrollment 流程

1. Local Master／Worker 啟動自己的 UI／control surface，使用者登入取得 JWT。
2. Local process 帶 bearer JWT 呼叫 Rust Website API 的 `/api/vpn/config`，取得 Headscale `auth_key`。
3. 在 process memory 使用一次性 key，建立或恢復 overlay identity，完成 Nodepool gRPC readiness handshake。
4. Master 可開始任務操作；public-dynamic Worker 另經 `/api/enrollment/credential` 與 `/api/enrollment/redeem` 取得 owner-bound client／Worker identity，再向 Nodepool 註冊。
5. Nodepool 驗證 Worker owner、執行身分與 runtime eligibility。

Identity enrollment credential 與 Headscale `auth_key` 是不同憑證：前者用於平台身分的單次兌換，後者用於加入 VPN。

當前 managed admission default 是 `public_dynamic`，不要求普通 Worker 使用者逐台加入 static cryptographic allowlist。Nodepool 驗證 bounded runtime／readiness report、保存 canonical capability digest 與 server observation timestamp；scheduling 仍檢查 readiness／digest／freshness（當前 managed check ≤30 秒）、reputation、budget、resources、slots 和 execution endpoint。它不是硬體 attestation，也不自動授權 GC／GPU operator capabilities。

API registration accepted、UI setup ready、Worker runtime ready、selected replica 和已達 quorum 是不同階段，不應在文案或 automation 中合併成一個「可以執行所有任務」狀態。

`WEBSITE_API_BASE` 或 role-specific `MASTER_WEBSITE_API_BASE`／`WORKER_WEBSITE_API_BASE` 必須指向真正提供登入與 protected `/api/vpn/config` 的部署 origin。不要只因官網能登入就把 Next BFF 當成 enrollment API。

### 6.3 重新啟動與 unattended startup

Client 會嘗試使用持久化的 libtailscale state。失效／撤銷的 overlay state 需要適當 session 或重新登入來取得新的一次性 material；expired JWT 不能用保存的 password 或 reusable key 自動繞過。

Operator 可選擇使用 role-scoped `MASTER_VPN_AUTHKEY`／`WORKER_VPN_AUTHKEY` 等 unattended startup 設定。這些是 operator credentials；自動加入需同時提供 login-server address 與 enrollment material。

Readiness 有 bounded timeout；VPN 不可用時應 fail closed，而不是改走另一個未驗證 endpoint。直接 gRPC、port forwarding 或已 enrolled peer 可以診斷部分問題，但不是完整 authenticated external enrollment 證據。

### 6.4 本機 session 與 credential 安全

本機 UI 的 bearer session 與 native bridge 有自己的邊界；不要將它誤描述成 WebView 自動具有 Nodepool 權限。

- 官網 token 僅存於 memory；Master／Worker token 存於 tab-scoped `sessionStorage`，可在同一 tab reload restore。三者不把新的 bearer 存入 `localStorage`，也不使用 httpOnly cookie 或 OS credential vault；theme preference 另行持久化。
- Browser logout 只清 UI session，不是 server-side token revocation，也不會自動停止 Worker 已啟動的 backend loops／執行分享。
- User JWT default TTL 是 24 小時，依 configuration 可變；不是永遠有效。現有 `RefreshToken` RPC 重編相同 claims，沒有延長 `exp`；HTTP surfaces 也沒有一般 refresh route。
- Master 本機 middleware 會讀 claims／expiry 做 routing／quota，不持平台 signing secret；Worker local status 某些路徑只看 Bearer presence。最終 authorization 仍須 Nodepool 驗證，不能把 local UI accepted state 當簽章已驗證。
- 不保存 password、reusable Headscale key 或平台私鑰。
- Windows singleton／named-pipe protocol 主要限制到同一 logon SID／role／UI port，**不驗證 executable identity**；same-logon malicious process 不在它的對抗保證內。
- Native SHOW／Quit／window actions 不可被擴展成任意 host command surface。
- 隱藏 UI 不會停止 Worker；退出時須走 client lifecycle confirmation。

更多 native session／視窗細節見 [SHADCN_UI.md](SHADCN_UI.md)。

<a id="managed"></a>

## 7. Managed-function-v1 任務與 DSL

### 7.1 新任務的 HTTP payload

```json
{
  "task_id": "example-sum-001",
  "runtime": "managed-function-v1",
  "task_source": "fn sum(values) { return get(values, \"a\") + get(values, \"b\"); }\nsum(input);\n",
  "torrent": "{\"a\":1,\"b\":2}",
  "memory_gb": 1,
  "cpu_score": 1,
  "storage_gb": 1,
  "max_cpt": 1000
}
```

重要說明：

- `task_id` 由 API caller 提供；UI 可以自行生成 ID。重複 submission 不能假定一定 idempotent，需遵循 server response。
- `runtime` 明確寫 `managed-function-v1`。
- `task_source` 是 DSL source text，不是 filename、ZIP、Python executable 或下載 URL。
- **`torrent` 是內含 JSON 的 string 欄位**；在 managed route 中代表 input data，不是 BitTorrent package。
- Resource fields 是 submission requirements，不是實際計費 operation，也不是硬體能力證明。
- `max_cpt` 是**整個任務含全部 replicas 與 fee 的收費上限**。
- API-level validation 成功之後，Nodepool 仍會做 authoritative admission；不是所有 syntactically valid requests 都能派發。

`201` submission response 不直接包含 completed task snapshot。必須查詢 task status，不能拿 HTTP create success 當 execution success。

### 7.2 語法概觀

```text
let name = expression;
fn name(arg1, arg2) { return expression; }
for item in expression { statements... }
return expression;
print(expression);
expression;
```

Expression 包含 integer、boolean、string、list、dict、name／call、parentheses、arithmetic／comparison 與 `if ... else ...`。最後的 expression statement 是 final value，除非較早 `return`。

例子：

```text
fn choose(values) {
  let a = get(values, "a");
  let b = get(values, "b");
  return if a > b { a } else { b };
}
choose(input);
```

`input` 是提交的 JSON data。函式作用於 runtime values，不可呼叫 host filesystem／network／process。

### 7.3 型別、builtins 與語言限制

CPU v1 支援 signed `i64` integer、boolean、string、list、string-keyed dict 和 `null`。CPU JSON input 的 numeric 欄位只接受可表示為 `i64` 的整數；共用 `Value` enum 的 `Float` 不用於此 runtime。

| 形式／builtin | 行為／限制 |
| --- | --- |
| `let x = expr;` | 宣告／重新綁定；不支援 bare `x = expr;` |
| `fn name(args) { ... }` | 頂層 named user function；受 call-depth／budget bounds |
| `if condition { expr } else { expr }` | Expression；需 `else`，branch 是單一 expression，不是 C-like statements block |
| `for item in values { ... }` | 只迭代 list；沒有 range／dict／string iterator |
| `and`／`or`／`not` | Boolean expressions；不是 JavaScript `&&`／`||` 語法 |
| `+ - * /` 與比較 | Signed integer arithmetic；checked overflow，division by zero 失敗 |
| `target[index]` | List／string 可用負 index 從尾端算，越界失敗；dict 用 string key |
| `list_or_dict[index] = value;` | 可對 simple variable target 賦值；不能假定 arbitrary nested target 都可改 |
| `len(value)` | String **UTF-8 byte length**、list item count、dict entry count |
| `get(target, key)` | Dict＋string key／list＋非負 integer index；missing／out-of-range 回 `null`；不支援 string `get` |
| `contains(target, value)` | Dict key membership、string substring、list equality membership |
| `print(expr);` | Parser special statement；不是可任意傳遞的一般 builtin；計入 bounded output |

注意 `len(string)` 計 bytes，而 `string[index]` 按 characters，兩者不能互相推算。

**Source string 的 Unicode 邊界不能忽略：** Lexer 支援 ASCII identifiers、雙引號與 `\"`、`\\`、`\n`、`\r`、`\t` escapes，不接受 `\uXXXX`。其 source literal byte-wise decoding 不保留原始 non-ASCII UTF-8 文字。因此不要直接在 DSL source literals 放中文等資料並承諾原樣 round-trip；需要 Unicode 的 data 應置於 JSON input，並以當前 parser／renderer 測試為準。

以下不在 CPU v1 語法／host capability surface：

- `while`、`break`、`continue`、`%`、member-dot access。
- Comment syntax：lexer 只略過 whitespace；範例不要加入 `//`、`#` 或 `/* ... */`。
- Import、filesystem、network、environment variables、subprocess、eval、reflection、DLL／native API。
- CPU math builtins `abs`、`min`、`max`、`sqrt`、`floor`、`ceil`、`round`、`exp`、`ln`、`pow`；相關 float-enabled 實作屬於另一個 explicit context，不能直接列成 CPU v1 API。
- GPU kernels／task-provided executable。

可核對的 tracked source example：[examples/managed-add.hdsl](../examples/managed-add.hdsl)：

```text
fn add(a, b) { return a + b; }
let total = add(get(input, "left"), get(input, "right"));
return {"total": total};
```

配合 input `{"left":1,"right":2}`，output 為 `{"total":3}`。更多 v1 examples 見 [templates/managed-function-v1](../templates/managed-function-v1)。

### 7.4 Structural／protocol limits

| 控制 | 當前 bounds／語義 |
| --- | --- |
| Submission source | 非空、最多 65,536 bytes（64 KiB） |
| Submission input string | 非空、最多 1,048,576 bytes（1 MiB） |
| Per-replica usage | Nodepool 從 task cap 推導的正整數 allowance |
| User function call depth | 64 |
| Interpreter output | 1,048,576 bytes |
| Single materialized value | Canonical logical JSON bytes ≤ 1,048,576 |
| Collection direct elements | ≤ 100,000 |
| Value nesting depth | ≤ 64 |
| Cumulative materialization | ≤ 16,777,216 logical bytes |
| Consensus typed result | Encoded envelope protocol cap 262,144 bytes；另受 configured bound |
| Round timeout | Default 120 秒；operator policy 可覆寫 |

Runtime logical memory accounting 計量 interpreter values，不限制 OS RSS／physical memory；resource requirement 也不會為每個 interpreter 欄位建立 OS quota。Materialization bytes 不另計 usage operations。

Interpreter output 上限為 1 MiB；consensus transport 的 encoded result cap 為 256 KiB，且包含 metadata。實際 output 應小於 wire cap。

提交時檢查 source／input 大小、runtime 與 budget；DSL 語法、JSON numeric type 或 evaluator 錯誤仍可能到 Worker 執行時才發現。

常見 runtime failure 包含 `budget_exhausted`、`integer_arithmetic_overflow`、`type_error`、`index_error`，以及 parse、call-depth、value／output／deadline 相關失敗。精確 code set 依 enforcing source 與 versioned tests，不能任意從 v0 failure contract 推成 v1。

### 7.5 計量不是「一行程式一個 CPT」

每個執行到的 statement／expression、argument、call overhead、print、loop body 等都可以消耗 operations。Literal、variable read 與它們外層的 statement 也不是免費；AST 子 expression 會分別計量。

在 v1，**一個實際執行 operation 對應一個 replica usage CPT**。不要用 source 行數、語法關鍵字個數或 wall time 推估精確 charge。Nodepool 派發的每個 replica allowance 是從 task-wide cap 推導，不是直接把 caller 的 `max_cpt` 當每個 Worker 的額度。

v1 不另設固定 `max_ops`／`max_loop_iterations` 取代 budget；call depth、value／materialization／memory／output 與 deadline 仍有 bounds。

精確 cost vectors 和語言範例見 [Managed Function Runtime](MANAGED_FUNCTION_RUNTIME.md) 與 versioned semantics manifests。Immutable manifest 中的 runtime allowance 名稱，不能覆蓋較高層當前 submission 的 task-total billing contract。

### 7.6 歷史 v0 相容範圍

v0 manifest／metering／billing／canonical evidence 保持 frozen，以便讀取舊資料。不能原地改寫它的語義，不能讓 unfinished v0 resume，也不能以 v0 receipt 直接套用 v1 的 usage billing。

歷史已 settlement 的 v1／v0 records 也不會因新 task-wide cap 自動重新計算退款。使用者查看舊任務時應核對當時的 billing version，而不是只看 runtime ID。

<a id="settlement"></a>

## 8. 計量、共識、CPT 與結算

以下共識與計費規則適用於 CPU `managed-function-v1`；GPU 與 general compute 的獨立契約見[第 10 章](#other-runtimes)。

### 8.1 CPT 的意義

CPT 是內部 quota／budget unit，不是貨幣。本 repository 有 account／ledger／transfer／earnings 與 quote 相關功能，不因此建立法幣、token conversion 或 withdrawable income。文件不提供兌換比率或財務收益承諾。

### 8.2 Task-wide cap 如何分配

新 v1 billing identity 是 `managed-function-v1-billing-v2-total-budget`；execution cost model 是 `managed-function-v1-execution-cpt-v1`，basis 為 `accepted-replica-executed-ops`。

令 task cap 為 `C`、replica count 為 `R`。Nodepool 求最大的正整數 `a`，使：

```text
reserved_usage = R × a
reserved_fee   = floor(reserved_usage / 10)
held_total     = reserved_usage + reserved_fee
held_total <= C
```

`a` 是每個 replica 相同的 execution allowance。若 cap 連每個 replica 的正額度都分不出來，admission 不能假裝成功。

**例：`C = 100`、`R = 3`**

- 每 replica allowance = 30。
- Reserved usage = 90。
- Reserved fee = 9。
- Held total = 99；沒有 hold 的 1 CPT 保留在 owner balance。

假設三個 replicas 最終各有 10、10、8 個有效 accepted operations：

```text
U       = 10 + 10 + 8 = 28
fee     = floor(28 / 10) = 2
charged = 28 + 2 = 30
refund  = 99 - 30 = 69
```

Provider payout 等於各 replica 的有效 usage；平台費是加在 aggregate usage 上的整數向下取整 10%，不是先對各 provider rounding，也不是從 provider payout 再扣 10%。

Enforce attempt creation 在 transaction 中建立 hold 並調整 payer balance；settlement 不對同一 hold 再扣一次，而是寫帳務、credit providers、refund unused hold。Invalid financial state／provider account／overflow 等檢查不成立時，transaction 不能假裝已完成。

每個 receipt 需綁定當前 task／attempt／execution／request／Worker、runtime／backend／semantics 與 source／input identities，並符合 terminal state、canonical result、output digest／length、counter consistency 和 allowance bounds。`usage_units` 與 `executed_ops` 必須相符；不是只取 Worker 自報數字就付費。

**資料欄位名可能有歷史語義：** `managed_consensus_usage_holds.max_cpt` 保存 per-replica allowance，外部 `tasks.max_cpt` 才是新 submission 的 task-total cap；不要把兩個欄位在報表／工具中不加區分地顯示為同一上限。

Historical `managed-function-v1-billing-v1` holds 有自己的相容處理；已入帳歷史資料不因新 contract 自動 recalculation／refund。

### 8.3 Consensus 的成功條件

預設 policy：

```text
MANAGED_CONSENSUS_ROLLOUT_MODE=enforce
MANAGED_CONSENSUS_REPLICA_COUNT=3
MANAGED_CONSENSUS_QUORUM=2
MANAGED_CONSENSUS_TIMEOUT_SECS=120
MANAGED_CONSENSUS_MAX_RESULT_BYTES=262144
```

- Replicas 必須是不同 assigned Worker identities；同一 Worker 重複回報不能湊 quorum。
- Token／request 綁定 task、execution、round、replica、attempt、idempotency、request digest 和 protocol。
- Nodepool 從 bounded canonical result bytes 自己計算 digest；不是相信 Worker 送來的 result hash。
- Usage diagnostics 不應把同一 deterministic output 分成不同 quorum groups。
- Certificate 與 durable observations／assignment／policy 的 binding 決定 successful output。
- Replica 不足或結果不一致時，不會自動降低 quorum 或使用 generic single-Worker completion。

不同 Worker ID 僅區分 Worker identity，不代表不同 owner／operator；colluding majority 或 common-mode runtime bug 仍可能一致回報錯誤結果。Consensus 提供 agreement evidence，不是 zkVM proof 或 resource attestation；新 managed 任務不需另一個 correctness prover 才能使用此共識路徑。

### 8.4 Rollout 模式

| 模式 | 行為 | 帳務／成功 output |
| --- | --- | --- |
| `enforce` | 按 persisted policy 建 round、dispatch、驗證結果與 usage | Success 需 quorum；有效 usage 才能依規則結算 |
| `observe` | Fan-out 並記錄 shadow evidence／quorum 或 no-quorum | Non-settling `OBSERVED`；不建立 usage hold／ledger mutation，不發布 billable successful result |
| `disabled` | Consensus route 關閉 | 不能把 policy-backed managed task silently downgrade 到舊 completion |

Runtime／policy 缺失或不匹配也應 hold／fail closed，而不是以修改 rollout mode 取得成功結果。

### 8.5 成功 output 與 payment eligibility 分開

- Quorum certificate 選出可發布的成功 output。
- Valid assignment-bound usage evidence 決定可計費 work。
- Failed／budget-exhausted partial work、divergent replicas 與 no-quorum rounds 仍可能有符合條件的 billable usage。
- Missing、invalid、stale／wrong-attempt receipts 不可計費。
- Receipt-only usage claim、generic completion 或自行宣告 success 不能取代 Nodepool 的驗證。
- Usage-only terminal settlement 不可捏造 certificate，也不可把 task 標成成功完成。


### 8.6 Stop／cancel／timeout 與 durability

取得 quorum、timeout 或 cancel 後，outstanding replicas 必須被 fencing，並收到 attempt-bound stop request。若不能確認停止，Nodepool 保留 `stop_pending`／`cancel_pending` 及 slot reservation，後續重試 stop，而不是把仍可能執行的 Worker slot 馬上重派。

新 v1 automatic paid retries 關閉；沒有 funded platform treasury 之前不把 automatic retry work 轉嫁給 owner。Stop request retry／恢復清理與「再次收費重跑任務」是不同事情。

持久化資料包括 task／policy、round、replica assignment、endpoint／provider snapshots、observations、slot reservations、certificate 與 settlement／ledger。正確診斷需跨這些 records 查看，而不只是檢查 Worker log 裡的 `completed`。

<a id="api"></a>

## 9. HTTP／gRPC 介面與操作範例

### 9.1 API 使用原則

- Local Master base、Local Worker control base、Official Site origin 與 Rust Website API origin 不可互換。
- Browser 使用 HTTP API；Nodepool gRPC 是 server／client transport，不是 browser public endpoint。
- 常見 auth header 是 `Authorization: Bearer <token>`；不要把 token 寫進 URL／query、文檔或公開 log。
- HTTP 200／201 不保證 body `success`，更不保證 execution／settlement success。
- Login response 的 token field 是 `token`，不是 `access_token`。Token expiry 或 401 要重新登入；本範例不自動重試提交或忽略 auth failure。
- Task owner checks 在 Nodepool；admin routes 另有 operator-configured permission。

### 9.2 核心 HTTP routes

下表以 `{task_id}`／`{worker_id}` 表示需替換的 path parameter。請依服務角色與認證方式呼叫相應 route。

#### Official Site：Next BFF

| Method | Path | Auth／作用 |
| --- | --- | --- |
| POST | `/api/register` | Account registration；success 201，business rejection 400 |
| POST | `/api/login` | 帳密登入；success 200，business rejection 401；回 `token` |
| GET | `/api/balance` | Bearer 轉 Nodepool；owner 由 token 決定 |

其他 task／worker／transfer／VPN／enrollment paths 不會自動代理，未支援的 path 回 404。Next BFF 沒有一般 refresh／cookie session endpoint。BFF 的 string-field helper 會 trim password，因此不要聲稱所有 login surfaces 對前後空白的處理完全相同。

#### Rust Website API

| Method | Path | Auth／作用 |
| --- | --- | --- |
| GET | `/health` | Public `OK` response，不是完整 dependency readiness |
| POST | `/api/register` | Public account registration |
| POST | `/api/login` | Account login，回 `token` |
| GET | `/api/balance` | Signed JWT＋Nodepool account authority |
| POST | `/api/transfer` | Signed JWT＋Nodepool；body `to_username`、`amount_cpt`、optional `idempotency_key` |
| POST | `/api/vpn/config` | Protected provisioning；含 sensitive one-time join handoff |
| POST | `/api/enrollment/credential` | Protected credential issue；body `role`／`client_instance_id` |
| POST | `/api/enrollment/redeem` | 以 credential 本身授權、single-use redemption；非一般 bearer middleware |

Identity enrollment credential 的 TTL 為 10 分鐘，DB 保存 hash／expiry／redemption state；以 transaction 鎖住兌換並綁定 stable client identity。`/api/vpn/config` 另提供加入 VPN 所需的 `auth_key`／`config_text`；這些是敏感資料，local status 不回 raw key。

#### Master HTTP API

| Method | Path | Auth／作用 |
| --- | --- | --- |
| GET | `/health` | Public `OK`；handler 不檢查 Nodepool 可達性 |
| POST | `/api/login` | Account login／適用的 enrollment flow |
| POST | `/api/register` | **410 Gone**；到官網註冊 |
| POST | `/api/vpn/bootstrap` | Bearer；本機 bootstrap／readiness |
| GET | `/api/vpn/status` | Bearer；non-secret local status，不是 Nodepool permission proof |
| GET | `/api/balance` | Bearer／Nodepool owner account |
| POST | `/api/tasks/quote` | Bearer；resource quote／preflight 資訊，正式 submission 仍重新檢查 |
| POST | `/api/tasks` | Bearer；建立 task，**不是同步 execution result** |
| GET | `/api/tasks` | Bearer／owner-scoped task list；沒有獨立 `GET /api/tasks/{task_id}` |
| GET | `/api/tasks/{task_id}/log` | Bearer／task owner；stored output |
| GET | `/api/tasks/{task_id}/result` | Bearer／task owner；result reference／typed GPU result／指引 |
| GET | `/api/tasks/{task_id}/artifact/download` | Bearer／owner 或 admin；binary attachment，optional `artifact_key` |
| POST | `/api/tasks/{task_id}/stop` | Bearer／owner；記錄 cancellation 並請求 stop，不保證立即全部停止 |
| POST | `/api/tasks/{task_id}/general-compute/artifacts/chunk` | Advanced task／manifest-authorized source artifact upload |
| GET | `/api/workers` | Bearer；普通使用者看自己的 Workers，admin 可看全部；**不是 public pool catalog** |
| POST | `/api/register-worker` | Compatibility／manual `PrivateStatic` flow；非該 admission mode 回 403 |
| POST | `/api/remove-worker` | Bearer／owner 或 admin；Master UI 目前沒有對應操作 |

Provider／admin 類別另有實際 API：

- `/api/provider/earnings`。
- `/api/provider/workers/{worker_id}/settings` 的 GET／PUT：enable、resource limits、minimum price。
- `/api/provider/workers/{worker_id}/trust`。
- `/api/admin/billing/overview`、artifacts overview／cleanup。
- Scheduling cache metrics／alert／anomalies、Worker trust control／list、audit logs。

完整方法以 [Master router](../hivemind-rs/crates/master-api/src/routes.rs#L15) 為準。Admin permission 依 Nodepool 的 username allowlist，不是看一般 JWT 自報 admin role；provider 操作通常 owner 或 admin。某些 business rejection 仍是 HTTP 200／`success:false`，必須讀 body。

Artifact cleanup handler 目前清除 expired artifact **DB rows**；不應宣稱它已刪除所有實際 artifact files 或完成 storage garbage collection。

#### Worker 本機 control HTTP

| Method | Path | Auth／作用 |
| --- | --- | --- |
| GET | `/api/worker-info` | Public detected／startup profile、Worker ID／callback |
| POST | `/api/login` | Account login／適用的 bootstrap flow |
| POST | `/api/vpn/bootstrap` | Bearer presence＋後續 authority flow；本機 setup |
| GET | `/api/vpn/status` | 只檢查本機 Bearer presence；不驗證本機 JWT signature |
| POST | `/api/register-worker` | Bearer／Nodepool token＋owner；註冊後啟動 backend loops |
| GET | `/api/worker-dashboard` | Bearer／Nodepool exact Worker owner；bounded host sample／assignments／settled credits |

Worker router **沒有 `/health`、pause／stop／remove、pricing 或 resource-settings endpoints**。`/api/startup-status` 由主 binary 的 LocalUiServer wrapper 提供，不是 Nodepool 健康檢查。

Dashboard telemetry 是 observation，不是 settled usage。Unknown／unsupported values 不能當成實測 0；settled credits 是帳號所有 Workers 的已結算 credits，不是目前這一台 Worker 的單機收入。

#### Submission defaults 與拒絕條件

| 欄位／狀況 | 實際規則 |
| --- | --- |
| `task_id` | Trim 後非空、最多 255 bytes；ASCII alphanumeric 或 `-_.`；禁止 `.` 與任何 `..` |
| Resource scores／GB fields | Optional omitted=0；不可負數 |
| `host_count` | Default 1，至少 1；不是 consensus replicas |
| `location` | Default `local` |
| `managed_consensus_version` | Optional；目前 HTTP 不接受 >1 |
| `managed_replica_count` | Optional；有提供時 2–7；省略向 Nodepool 送 0，由 policy 決定 |
| `managed_quorum` | Optional；positive 且與 replicas 同時提供時需 strict majority／不超 replicas |
| `max_cpt` | V1 positive signed-i64 integer；不應依賴省略後的 generic quote fallback |
| Unknown fields／錯誤 JSON 型別 | `deny_unknown_fields`／extractor rejection，通常 422；不是所有錯誤都回 400 |
| ID／runtime／resource validation | 400 |
| Quote 超過 cap／insufficient balance | 402 |
| Duplicate task ID | 409 |
| Submission quota | 429 |
| Successful create | 201，body `success:true`、`task:null` |
| Auth／VPN／transport failure | 依分支 401／502／503／500；不可當做 execution outcome 已確定 |

Rate limiter 是 Master process-local、per-owner 60 秒 window，預設 60/min。設定 `HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE <= 0` 會封鎖所有提交，不是關閉限制。Handler-level invalid request 也可能先消耗 quota；schema extractor rejection 則尚未進 handler。

Task create 的 HTTP body cap 為 21 MiB，因 router 同時支援 larger advanced manifests；CPU v1 的 source／input bounds仍各自較小。這不能解讀成可提交 21 MiB managed source。

### 9.3 gRPC 與 protobuf 分工

Canonical definitions 位於 [proto/hivemind.proto](../proto/hivemind.proto) 與 [proto/vpn.proto](../proto/vpn.proto)。Build 使用 tonic／prost 產生 bindings；不要手動編輯 generated output。

| Contract 群 | 重點 |
| --- | --- |
| `UserService` | Account register／login／balance／transfer／enrollment；不是 browser 可直接使用的 public transport |
| `MasterNodeService` | Token-bound task quote／upload／list／result／log／stop／download，provider／admin 類別 |
| `NodeManagerService` | Worker registration／ownership／status／capability；resource reports 不是硬體 attestation |
| `WorkerNodeService` | Nodepool-issued execution token；task／execution／attempt／digest binding與 stop／result |
| GC artifact／chunk services | User→Nodepool source upload 與 Nodepool→Worker transfer 是不同 auth／manifest／lease 邊界 |
| `WorkerSessionService` | Bidirectional gRPC session，與 browser WebSocket 分開；active consensus 使用 unary callback |
| `VPNService` | Onboarding config 與仍存在的 compatibility mesh contracts；UI不因此取得VPN管理權 |
| `BatchRuntimeService` | 有 server wiring／handler，但不是目前三個 UI 的主要 task workflow |

Proto 保留歷史 `torrent` 與 package 欄位；active HTTP v1 使用 source／JSON input，不接受任意 ZIP 或 executable。

### 9.4 PowerShell 提交示例

執行前需確認 Master API 已啟動並完成 enrollment／Nodepool readiness、帳號已建立且餘額足夠，並有至少三個符合預設 managed policy 的 eligible Workers。

```powershell
$master = "http://127.0.0.1:8082"

$credential = Get-Credential -Message "Hivemind account"
$loginBody = @{
    username = $credential.UserName
    password = $credential.GetNetworkCredential().Password
} | ConvertTo-Json -Compress

$login = Invoke-RestMethod -Method Post -Uri "$master/api/login" `
    -ContentType "application/json" -Body $loginBody
if (-not $login.token) { throw "Login did not return a token" }
$headers = @{ Authorization = "Bearer $($login.token)" }

$taskId = "manual-" + [Guid]::NewGuid().ToString("N")
$payload = @{
    task_id = $taskId
    runtime = "managed-function-v1"
    task_source = 'fn sum(values) { return get(values, "a") + get(values, "b"); } sum(input);'
    torrent = '{"a":1,"b":2}'
    memory_gb = 1
    cpu_score = 1
    storage_gb = 1
    max_cpt = 1000
} | ConvertTo-Json -Depth 10 -Compress

$created = Invoke-RestMethod -Method Post -Uri "$master/api/tasks" `
    -Headers $headers -ContentType "application/json" -Body $payload
if (-not $created.success) { throw "Task submission was rejected" }

# API 僅提供任務清單，需以 task_id 篩選。
$terminal = @("COMPLETED", "FAILED", "CANCELLED", "TIMED_OUT", "OBSERVED")
$deadline = (Get-Date).AddMinutes(5)
$finished = $false
while ((Get-Date) -lt $deadline) {
    $list = Invoke-RestMethod -Method Get -Uri "$master/api/tasks" -Headers $headers
    if (-not $list.success) { throw "Task listing was rejected" }
    $task = $list.tasks | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
    if ($task -and ($terminal -contains $task.status)) {
        $finished = $true
        break
    }
    Start-Sleep -Seconds 2
}
if (-not $finished) { throw "Polling deadline exceeded; outcome remains unknown" }

# COMPLETED 後可能仍在等待其他副本停止，尚未結算。
$task
Invoke-RestMethod -Method Get -Uri "$master/api/tasks/$taskId/log" -Headers $headers
```

登入 response 含 bearer token，任務 response 可能含使用者資料，避免寫入一般日誌。

### 9.5 結果、log 與 artifacts

**Managed output 的選擇規則：** 若 `print` output 非空，該 output 就是 Worker 傳回的主要結果，不再自動附加 final return value。若沒 print output，top-level string render 為 raw UTF-8，其他 values render 為 compact JSON，dict keys 使用 deterministic order。

`GET /api/tasks` 有 stored output 和 observability 欄位，包括 status／message、dispatch／Worker／provider、usage、cap、`billed_amount`、`billing_settled`、retry count 與 managed consensus summary。這些欄位需一起判讀：

- `COMPLETED` 後，未確認的 replica stop 可能延後 `billing_settled:true`。
- `retry_count`／derived `dispatch_status` 是摘要欄位；查閱 attempt／settlement records 確認實際重跑與收費。
- `managed_consensus` 公開 votes／certificate／mode／settlement 摘要；raw replica transport evidence 不會全部公開。
- Owner-only list 不是匿名任務 feed。

`GET .../log` 的 body 是 `{success, task_id, log}`，其中 `log` 為 stored `task.output`，不是 realtime stream。`GET .../result` 的典型 body 有 `success`、`task_id`、`result_torrent`、`status_message`；它不是通用 stdout route：

- 未完成、failed／cancelled／timed-out 或 `OBSERVED` 回 `success:false`。
- `COMPLETED` 且真的有 result torrent reference 才可在該 legacy reference contract 成功。
- 完成的 managed 任務若沒有 torrent，可能 HTTP 200／`success:false`，並引導到 log／artifact。
- Typed GPU result 有自己的 object／status，不能用有 object 就認定它是 success。
- 部分 owner mismatch／not-found business cases 仍可能 HTTP 200／`success:false`。

輸出 retrieval、artifact bytes 和 receipt／certificate／billing evidence 是不同 contract。Download handler 是 binary attachment；不要用 `Invoke-RestMethod` 的 JSON 假設解析它。取得輸出前確認 owner／artifact key／status／size 等 server response。

**沒有通用 secret scrubber 保證。** 使用者 output 可以原樣持久化／回傳；不要在 source、input、print 或 return value 放 credentials 並期待系統遮蔽。

### 9.6 Worker callback 與可達性

Worker registration 中的 callback 是讓 Nodepool scheduler 發起 execution RPC 的 target；它不是 caller identity。Token／owner／assignment 驗證和 network reachability 是兩件事。

- 不要註冊空 target、`0.0.0.0`、unspecified host 或 Nodepool 無法 route 的 localhost。
- **Active managed consensus 目前走 Nodepool 主動連 Worker 的 unary gRPC callback。** Session-only／outbound registration 成功不能證明這條 v1 execution 路徑可達；non-consensus session-first branch 不能拿來宣稱 v1 已完成 NAT traversal。
- Container callback、host callback 和 Headscale overlay address 要與實際部署相符。
- 目前的 endpoint checks 不足以被描述成完整的 IP／DNS／SSRF allowlist 或資源 attestation。
- Operator 應另行限制控制面的可訪目的地、egress 與 network segmentation；不要只因 registration accepted 就開放任意 internal address。

<a id="other-runtimes"></a>

## 10. GPU、General Compute 與 ONNX

### 10.1 不同 runtime 不能混為一談

| 路徑 | 主要特性 | 額外條件／邊界 |
| --- | --- | --- |
| `managed-function-v1` | Deterministic closed JSON DSL | Budget、structural limits、replicated consensus；無 arbitrary host APIs |
| `managed-function-gpu-v1` | 固定 Rust-owned GPU／math extension | Explicit GPU context、typed capability、相符 backend；不接受 task kernel／DLL |
| `general-compute-v1alpha1` | 一般程式／科學 workload 的 pre-release 契約 | Beta admission、operator-pinned assets、OS-specific production isolation |
| ONNX on general compute | Operator-owned runner／image 內執行 inference | Model／tensor ABI、provider／GPU pinning、artifact／output evidence |

### 10.2 Managed GPU extension

固定 operations 包含 `gpu_add_f32`、`gpu_scale_f32` 與 `gpu_matmul_f32`。Task 不能提供 CUDA C／PTX、pointer、device handle、kernel source、dynamic library 或 executable。

GPU-required request 缺少 trusted compatible GPU／backend 時 fail closed，不得默默使用 CPU reference backend 然後報成 GPU work。浮點／math surface 有自己的 semantics identity，不能改寫 frozen v0 的 integer result contract。

GPU 的完成與結算使用獨立路徑。Nodepool 依持久化的 request manifest、assignment 與 trusted capability snapshot 驗證 typed result，成功時在同一交易保存結果並完成 GPU 專用的 fixed-reservation 結算；不套用 CPU v1 的 quorum certificate 或按 operation 計費契約。

GPU readiness、driver／backend compatibility、實際執行與 Nodepool settlement 分屬不同檢查。

### 10.3 Linux general compute

Production mode 是 `production_sandboxed_oci`：要求 rootless OCI、user／pid／mount／network namespaces、cgroup v2、`no_new_privileges`、read-only root、明確 artifact／scratch mounts、deny-all network 及 digest-pinned default-deny seccomp profile。

Runner、image、bundle／rootfs、state root 與 policy 都必須是 operator-owned pinned assets。缺少、identity 不匹配或 host primitives 不可用時回 `backend_unavailable`，不能回到 direct host process。

OCI fixture 與 prerequisites 見 [OCI fixture guide](general-compute-oci-fixture.md)。

### 10.4 Native Windows general compute

Production mode 是 `production_sandboxed_windows`，以原生 Windows Containers／HCS provider 為邊界。Job Object 主要提供 lifecycle／process cleanup，單獨存在不夠證明隔離。

生產邊界還需 verified container、restricted identity、read-only root、safe explicit mounts、deny-by-default network、CPU／memory／process bounds、reparse-point-safe roots 和 hostile-workload evidence。

不得用 direct host process、`cmd.exe`、PowerShell、Docker CLI、WSL、Linux VM 或未受限 AppContainer 代替。**缺少 HCS 不阻擋 closed DSL；它只阻擋需要 HCS 的 general-compute backend。**

`scripts/windows-hcs-e2e.ps1` 目前檢查 prerequisites，並不自己執行 task。要聲稱原生 HCS workload pass，需額外保存真正的 HCS task execution evidence。

### 10.5 ONNX artifact 與 tensor 邊界

ONNX Runtime／TensorRT／CUDA 放在 operator-built guest image，不載入 task-provided native code 到 Worker host。

- Model source 是 verified artifact；不能提供 arbitrary URL 或 host path。
- External-data sidecar 不在當前 ONNX protocol 範圍；model 要以支援的單一 artifact 包裝。
- Input／output 使用 canonical tensor envelope，固定 name、dtype、shape、little-endian `data_hex`。
- Dtypes 包含 `float32`、`float64`、`int32`、`int64`、`uint8`、`bool`；NaN／Infinity 和非法 bool bytes 被拒絕。
- Rank、element count、payload size 有 bounds；無 implicit batching。
- Provider 必須實際啟用；宣稱 CUDA／TensorRT 不能容許 silently CPU fallback。
- Output file metadata 記錄 artifact 資訊；bytes 由 bounded inline verifiable payload 傳回，完整 result envelope 仍受 RPC size limit。

完整 tensor ABI、ordered mounts 與 runner registration 例子見 [GENERAL_COMPUTE_ONNX.md](GENERAL_COMPUTE_ONNX.md)。

<a id="operations"></a>

## 11. 正式部署、資料與維運

### 11.1 正式服務分割

專案部署邊界將 Orange Pi ARM64 作為 control plane：Nodepool、Rust Website API、Headscale、PostgreSQL、Redis。Master 與 Worker 放在其他適合的主機，不在 Orange Pi 執行。

Root all-in-one Compose 與 legacy VPN compose 僅供本機開發／smoke 使用。

### 11.2 Operator 上線檢查

1. Pin release commit／artifact／container digest 和適用 runtime／semantics／policy。
2. 準備唯一且足夠強的 JWT secret、matching execution key pair、database／Redis credentials 與 server-side Headscale secret。
3. 確認 browser-facing HTTPS origins、TLS／reverse proxy、role-specific upstream 和 allowed network paths。
4. 確認正式 Nodepool 只有經驗證 client transport 可用；不要直接暴露給 browser。
5. 確認 callback 可被 Nodepool route，但 Nodepool egress 不會任意訪問控制面敏感服務。
6. Provision durable Worker state root、operator backend registry 與安全檔案權限。
7. 確認 database migration、backup／restore 與 secret rotation 方法。
8. 註冊足夠 eligible Workers，分別驗證 provider、runtime identity 和 readiness。
9. 執行符合 release claim 的 account／enrollment／task／quorum／usage／settlement checks。
10. 保留去除 secrets 的版本、時間、環境與 gate evidence；未通過項目明確標 blocked／deferred。

不要只把 `.env.example` 複製成 `.env` 就認定設定安全；也不要把 root Compose 為共置開發傳給 Master／Worker 的平台 secret 當成正式 client provisioning 方法。

### 11.3 資料庫與 migration

Nodepool 的 durable records 是權威狀態。Migration 與 repository 邏輯共同決定 task／consensus／billing 的一致性；Redis cache 不是結算事實的替代品。

目前主要 schema upgrade 內嵌在 `database/src/postgres.rs`，以 SQL transaction 與 transaction advisory lock 協調 `CREATE/ALTER ... IF NOT EXISTS`、constraints、indexes／backfill，最後 commit。

核心資料類別包括：

| 記錄 | 用途 |
| --- | --- |
| `users`、`worker_nodes`、`tasks` | Balance、Worker identity／capability／readiness、task lifecycle |
| `enrollment_credentials`、`client_identities` | Credential hash／expiry／redemption 與 stable client identity |
| Consensus policies／attempts | Persisted policy、round／deadline／mode／state |
| Worker reservations／replicas | Slots、endpoint／provider／attempt snapshots、terminal observations |
| Consensus certificates | Matching output digest、participants、evidence level 與 authorization |
| V1 usage holds | Reserved／actual usage、fee／charge／refund／outcome；支援無 quorum usage settlement |
| Replica payouts／ledger | Per-replica eligibility、accepted usage、payment／rejection 和 idempotency |
| Reputation／attestations／audit | Server-side observations 與 trust decisions；不是通用硬體 attestation |

另有歷史命名陷阱：replica 的 `result_json` 欄位在這條 v1 路徑保存 encoded protobuf `BYTEA`，不是任意 JSON text。維運工具應按 typed contract decode，不能只依欄位名猜測內容。

資料庫升級應遵循適用的 migration runner，先 backup 再測試；不要手動修改 billable flags、certificate 或 ledger 來「讓任務完成」。相同 task／attempt 的重送應由 server-side transaction／idempotency 機制處理，不能透過複製 task rows 實現。

### 11.4 Backup 與 restore 建議

- 保存 PostgreSQL 一致性 backup，涵蓋 account、task、round／replica、certificate、usage／ledger、audit 和相關 schema。
- 依 artifact／state 保存方式一起保存需要的 bytes、manifests、registry／policy 和 Worker stop-fence state。
- JWT／execution／Headscale private credentials 使用另外的受保護 secret store，不與普通 log archive 混放。
- Restore 到隔離 staging 環境驗證 migration、auth、resume／fencing、result／settlement 一致性。
- 記錄 restore point、artifact version 和 keys 是否相容。不要讓恢復出的舊 state 接管仍在執行的 production attempt。

定期在隔離 staging 演練還原；`db-reset` 會重建資料庫與 Redis，不是還原程序。

### 11.5 Logging 與 observability

診斷保留 timestamp、service role、task／round／replica／attempt identifiers、bounded error code 和 relevant policy／backend identity。避免記錄 password、raw JWT、Headscale key、private key、完整 task source／input 或敏感 output。

Public task API、Master observability 與 Nodepool replica evidence 有不同存取範圍。API 設計應避免暴露完整 token、transport envelope 或 operator-sensitive provider data。

跨服務診斷應查閱 server-side audit／records；分享 log 前先檢查敏感資料。

<a id="testing"></a>

## 12. 測試、Release Gates 與證據

### 12.1 後端與 executor

從 repository root：

```powershell
cargo fmt --manifest-path hivemind-rs/Cargo.toml --all -- --check
cargo clippy --manifest-path hivemind-rs/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path hivemind-rs/Cargo.toml

cargo fmt --manifest-path executor-rs/Cargo.toml --all -- --check
cargo clippy --manifest-path executor-rs/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path executor-rs/Cargo.toml
```

這些是 baseline checks。`--all-features` 與各 OS-specific features 需另外建置／測試；Linux、Windows、GPU、native WebView 應在各自平台驗證。

### 12.2 真正的 settlement tests 需要 PostgreSQL

將 `HIVEMIND_TEST_DATABASE_URL` 設為專用 test database 後執行相關 cases；未設定時部分 test config 使用 `hivemind_test`。有些 tests 在資料庫不可用時會 skip 或提早返回。記錄 DB 可達性、migration 結果、實際執行的 cases 與 skip reason。結算測試需驗證 hold、receipt、certificate、ledger 與 refund 路徑。

**不要使用 production DB**：test setup 可能清除資料或重建 schema。

### 12.3 Frontend／文檔 checks

```powershell
npm --prefix frontend test
npm --prefix frontend/master-ui test
npm --prefix frontend/worker-ui test
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-docs.Tests.ps1
```

Docs script 直接執行 PowerShell assertions，不需要 Pester；它檢查指定文檔的 release／trust／billing 字句，不涵蓋所有 Markdown 事實、連結或 code examples。

Browser UI contracts、accessibility／theme checks、native lifecycle、three-surface service flow 和 external execution 都要分開記錄。更多命令見 [SHADCN_UI.md](SHADCN_UI.md) 與 [GETTING_STARTED.md](GETTING_STARTED.md)。

### 12.4 證據矩陣

| Check | 可確認的範圍 | 尚需其他證據的項目 |
| --- | --- | --- |
| Compile／package | Artifact 可建置及部分靜態條件 | Workload 執行、網路可達性、隔離、結算 |
| DSL semantic／limit test | 測試涵蓋的語法、determinism、bounds 與 budget semantics | 外部 replicas 執行、資料庫結算 |
| Database-backed test | 該環境的 transaction、hold、receipt、ledger assertions | 公網路徑、hostile provider security |
| Mock／fake runner | Protocol、routing、lifecycle 與錯誤映射 | 真正 OCI／HCS 隔離 |
| Local Compose health／HTML smoke | 已檢查的 services 與 responses | Account enrollment、三 replica success、外部 Headscale |
| Browser test | 實際執行的 UI／API assertions | 未執行的 assertions、其他 provider gates |
| OCI `-CheckOnly` | Prerequisites、assets、policy preflight | OCI task 執行與隔離效果 |
| Windows HCS prerequisites script | Feature、service、bundle prerequisites | HCS task 執行 |
| GPU／ONNX backend test | 指定裝置、driver、provider 與 workload 的結果 | 其他硬體、provider 或 release 組合 |
| External enrollment／task E2E | 受測部署路徑的 auth、overlay、replicas、result、usage、settlement | 其他 release／平台／規模／SLA |
| Signed update clean-host check | 該 artifact／channel 的 verify、activation、recovery | 其他 keyset／channel 的可用性 |

### 12.5 Release evidence 最低內容

- Commit／release ID、artifact digest、runtime／semantics／cost-model／policy version。
- OS／architecture、provider、driver／runtime、dependency versions。
- Network topology 與是否走真正 external Headscale enrollment。
- Database／migration 狀態、replica identity 與 callback reachability。
- Test command、timestamp、exit status、pass／fail／skip／blocked counts。
- Task／round／attempt、quorum、validated usage、hold／fee／refund／settlement 的去識別化證據。
- 對失敗／cancel／timeout／recovery 的結果，不只保存 happy path。

[SMOKE_BENCHMARKS.md](SMOKE_BENCHMARKS.md) 說明多 Worker 的測量程序與建議 criteria。

<a id="troubleshooting"></a>

## 13. 故障排查

| 現象 | 優先檢查 | 不應採取的捷徑 |
| --- | --- | --- |
| Binary 拒絕啟動 | Role config、JWT 長度／default、matching key、dependency readiness、registry／state permissions | 關閉 auth 或用預設 secret |
| `.env` 改了仍不生效 | 啟動入口的 dotenv 策略、工作目錄或執行檔位置；process env／JSON 與 Compose overrides | 假設 CLI 和 Windows package 讀取相同位置 |
| Compose 缺 mandatory variable | 全檔的 password／JWT／keys，哪個 shell／environment 供 parser 使用 | 以 public dummy secret 當正式值 |
| Worker 啟動失敗 | Backend registry path／JSON、durable state root、public key；Windows 可看 startup-status／failed UI | 缺 registry 時自動改成 direct execution |
| 官網 balance 無法連 Nodepool | Server-side BFF target；容器中 `localhost` 是否錯了 | 加 browser-side Nodepool public target |
| VPN enrollment 404／unauthorized | Origin 是否 Rust Website API，protected config route，JWT role／有效期 | 把 Next BFF login 當完整 enrollment |
| VPN 有 peer 但 task 不動 | Nodepool gRPC protocol probe、overlay routing、registration／capability／readiness | 只看 ping／SSH 成功就判 handshake 成功 |
| Worker 註冊成功但未 dispatch | Eligible runtime、capacity／policy、callback reachability、distinct replica 數量 | 把 registration 當 universal capability |
| 一直沒有成功 managed result | 預設是否有 3 eligible Workers／2 matching results；budget、deadline、identity binding | 降成單一 Worker completion |
| `max_cpt` 足夠卻 budget exhausted | 每副本 allowance 是除 fee 和 replicas 後的整數 budget；source operation cost | 把 task cap 當每個 replica cap |
| Failed／no-quorum 仍扣 CPT | 是否有 valid partial／divergent usage，billing version、hold／fee／refund | 直接把 failure charge 當帳務一定錯 |
| `/result` 200 但 `success:false` | 是否只有 legacy torrent result 可下載；查看 log／artifact／observability | 把 HTTP status 當 result success |
| Cancel 後 slot 沒立即釋放 | `stop_pending`／`cancel_pending`、attempt-bound stop confirmation | 強制重派尚未確認停止的 capacity |
| GPU backend unavailable | Feature、typed capability、trusted backend、driver／assets identity | Silent CPU fallback 並回報 GPU success |
| Linux OCI unavailable | Rootless／namespaces、cgroup v2、seccomp、pinned assets、safe mounts | 用 rootful／fake runner 宣稱 production pass |
| Windows HCS unavailable | Native feature／vmcompute／operator bundle 與 policy | 用 Job Object／WSL 代替原生 isolation |
| Browser QA 找不到服務 | Smoke 打印的動態 URLs、test shell 的 overrides | 固定猜 8080／3000／3001 |
| Workspace tests 綠但 settlement 未確認 | PostgreSQL reachable、test skip／early return、相關 cases output | 將 skip 計成有效 DB pass |
| 更新不可用 | Approved signed metadata／package／version／keyset、activation/recovery evidence | Unsigned fallback 或直接替換 binary |

診斷時先保留 logs／state，再決定需不需要重啟。不要先 `down -v`／`db-reset`／刪 Worker state，否則可能抹除 consensus、billing 或 stop-fence 證據。

<a id="limits"></a>

## 14. 限制、待驗證項目與授權

### 14.1 產品範圍

- 尚無成熟公開 marketplace、獨立 provider bidding／attestation、dispute resolution 或公開 SLA。
- CPT 不提供兌換或保證收益；外部支付機構未接通。
- 正式 client、native provider 與完整 E2E 的可用性依平台而異。
- Worker resource claims 不是硬體證明；不同 Worker ID 也不代表不同 operator。
- Consensus certificate 證明結果一致性，不證明語義正確或實際 usage。
- `general-compute-v1alpha1` 是 pre-release 契約，不是穩定的公開通用運算服務。
- Signed-update wiring 不等於正式發佈通道已驗證。
- 歷史文件中的 ZIP 任務、舊 allowlist、billing 與 retry 描述不適用於目前 managed 契約。

### 14.2 授權

專案採用 MIT [LICENSE](../LICENSE)。第三方依賴、vendored 元件與打包 runtime／image 依各自授權；重新散布時需保留相應授權文件與 notices。

<a id="sources"></a>

## 15. 原始碼與延伸文檔索引

索引路徑與行號對應本手冊所列程式碼版本；程式異動後請以 symbol／module 重新定位。

### 15.1 信任、角色、設定與 build

| 主題 | 原始碼與實作入口 |
| --- | --- |
| 官方／用戶部署與 trust model | [AGENT.md:16–43](../AGENT.md#L16)、[官網邊界](../AGENT.md#L74)、[native execution boundary](../AGENT.md#L173) |
| 角色與 secret validation | [bin roles:77–92](../hivemind-rs/crates/hivemind-bin/src/lib.rs#L77)、[role validation:201–213](../hivemind-rs/crates/hivemind-bin/src/lib.rs#L201) |
| JWT／Ed25519 key validation | [config:378–426](../hivemind-rs/crates/config/src/lib.rs#L378) |
| Defaults／env-loading route | [config defaults:527–596](../hivemind-rs/crates/config/src/lib.rs#L527)、[dotenv policy](../hivemind-rs/crates/config/src/lib.rs#L605)、[角色入口](../hivemind-rs/crates/hivemind-bin/src/lib.rs#L95)、[CLI 入口](../hivemind-rs/crates/hivemind-bin/src/lib.rs#L120)、[loader:662–786](../hivemind-rs/crates/config/src/lib.rs#L662) |
| Consensus defaults／validation | [config:88–178](../hivemind-rs/crates/config/src/lib.rs#L88) |
| Toolchain／CI／prerequisites | [Hivemind CI:59–83](../.github/workflows/ci.yml#L59)、[executor CI:94–136](../.github/workflows/ci.yml#L94)、[Windows:217–282](../.github/workflows/ci.yml#L217)、[Docker builder](../hivemind-rs/Dockerfile#L1)、[executor manifest](../executor-rs/Cargo.toml#L15) |
| `make dev` 與 platform scripts | [root Makefile:25–31](../Makefile#L25)、[frontend targets:55–62](../Makefile#L55) |
| Smoke ports／volumes／cleanup | [release-stack-smoke:203–234](../scripts/release-stack-smoke.ps1#L203)、[volumes:279–299](../scripts/release-stack-smoke.ps1#L279)、[branch／cleanup:397–457](../scripts/release-stack-smoke.ps1#L397) |
| Local startup gate | [LocalUiServer:45–204](../hivemind-rs/crates/hivemind-bin/src/local_ui_server.rs#L45) |

### 15.2 Managed DSL、admission、輸出與限制

| 主題 | 原始碼與實作入口 |
| --- | --- |
| V1／v0 admission、source／input bounds | [Nodepool grpc:805–952](../hivemind-rs/crates/node-manager/src/grpc.rs#L805)、[shared constants](../hivemind-rs/crates/proto/src/lib.rs#L28) |
| No paid v1 retries／task persistence | [grpc:2081–2167](../hivemind-rs/crates/node-manager/src/grpc.rs#L2081) |
| CPU input types | [runtime Value:66–75](../executor-rs/crates/managed-function-runtime/src/lib.rs#L66)、[integer-only input:478–581](../executor-rs/crates/managed-function-runtime/src/lib.rs#L478) |
| Lexer／parser／source literals | [lexer:1015–1128](../executor-rs/crates/managed-function-runtime/src/lib.rs#L1015)、[parser:1243–1615](../executor-rs/crates/managed-function-runtime/src/lib.rs#L1243) |
| Evaluation／metering | [statement accounting:1711–1825](../executor-rs/crates/managed-function-runtime/src/lib.rs#L1711)、[calls:2138–2200](../executor-rs/crates/managed-function-runtime/src/lib.rs#L2138)、[charge:2506–2538](../executor-rs/crates/managed-function-runtime/src/lib.rs#L2506) |
| Builtins／indexing／math guard | [indexing:2077–2135](../executor-rs/crates/managed-function-runtime/src/lib.rs#L2077)、[CPU builtins:2203–2286](../executor-rs/crates/managed-function-runtime/src/lib.rs#L2203)、[float guard](../executor-rs/crates/managed-function-runtime/src/lib.rs#L2385) |
| Checked integer arithmetic | [runtime:2547–2629](../executor-rs/crates/managed-function-runtime/src/lib.rs#L2547) |
| Logical structural limits | [limits:266–340](../executor-rs/crates/managed-function-runtime/src/lib.rs#L266)、[value bounds:1895–2009](../executor-rs/crates/managed-function-runtime/src/lib.rs#L1895) |
| Partial receipts／actual output | [Worker evaluator:209–332](../hivemind-rs/crates/worker-executor/src/executor.rs#L209) |
| Frozen／versioned semantics | [v0 manifest](../executor-rs/crates/managed-function-runtime/managed-function-v0-semantics.json)、[v1 manifest](../executor-rs/crates/managed-function-runtime/managed-function-v1-semantics.json) |
| Tracked starter example | [managed-add.hdsl](../examples/managed-add.hdsl#L1) |

### 15.3 Consensus、usage、billing 與 durable records

| 主題 | 原始碼與實作入口 |
| --- | --- |
| Canonical result／quorum binding | [managed-consensus:106–274](../hivemind-rs/crates/managed-consensus/src/lib.rs#L106)、[proto result bounds](../hivemind-rs/crates/proto/src/lib.rs#L80) |
| Active callback versus session branch | [dispatcher:545–669](../hivemind-rs/crates/task-scheduler/src/dispatcher.rs#L545)、[replica RPC:2522–2614](../hivemind-rs/crates/task-scheduler/src/dispatcher.rs#L2522) |
| Worker endpoint helper | [dispatcher:1232–1242](../hivemind-rs/crates/task-scheduler/src/dispatcher.rs#L1232) |
| Attempt-bound token | [auth worker_execution:147–219](../hivemind-rs/crates/auth/src/worker_execution.rs#L147)、[Worker response binding](../hivemind-rs/crates/worker-executor/src/grpc_server.rs#L1194) |
| Task-total budget／billing versions | [task_repository:429–647](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L429) |
| Fee／payout／refund | [task_repository:674–768](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L674)、[transaction:912–1054](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L912) |
| Valid receipt／all-replica settlement | [evidence checks:771–909](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L771)、[usage plan:1057–1184](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L1057)、[without-certificate settlement](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L2707) |
| Quorum stop-pending／recovery | [task_repository:3327–3597](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L3327)、[recovery:1754–1943](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L1754) |
| Schema transaction／core records | [postgres migration entry:145–176](../hivemind-rs/crates/database/src/postgres.rs#L145)、[account／task:178–320](../hivemind-rs/crates/database/src/postgres.rs#L178)、[consensus／usage:428–662](../hivemind-rs/crates/database/src/postgres.rs#L428)、[ledger:1297–1345](../hivemind-rs/crates/database/src/postgres.rs#L1297) |
| V0 retirement／historical preservation | [task_repository:7857–7939](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L7857) |
| Public-dynamic readiness／freshness | [registration service](../hivemind-rs/crates/node-manager/src/service.rs#L59)、[scheduler readiness](../hivemind-rs/crates/task-scheduler/src/scheduler.rs#L142)、[freshness check](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L1252) |

### 15.4 APIs、UI、session 與其他 runtimes

| 主題 | 原始碼與實作入口 |
| --- | --- |
| Master routes | [master-api routes:15–117](../hivemind-rs/crates/master-api/src/routes.rs#L15) |
| HTTP schema／ID／rate limit | [body fields](../hivemind-rs/crates/master-api/src/handlers.rs#L114)、[ID validation](../hivemind-rs/crates/master-api/src/handlers.rs#L454)、[limiter](../hivemind-rs/crates/master-api/src/handlers.rs#L51)、[create handler](../hivemind-rs/crates/master-api/src/handlers.rs#L1435) |
| Task list／result／log mappings | [HTTP task list](../hivemind-rs/crates/master-api/src/handlers.rs#L1557)、[HTTP log／result](../hivemind-rs/crates/master-api/src/handlers.rs#L1808)、[Nodepool result](../hivemind-rs/crates/node-manager/src/grpc.rs#L2250)、[Nodepool log](../hivemind-rs/crates/node-manager/src/grpc.rs#L2572) |
| Provider／admin／artifact cleanup | [Nodepool settings](../hivemind-rs/crates/node-manager/src/grpc.rs#L2780)、[cleanup](../hivemind-rs/crates/node-manager/src/grpc.rs#L3072)、[admin check](../hivemind-rs/crates/node-manager/src/grpc.rs#L4139) |
| Worker control routes／dashboard | [control router:348–381](../hivemind-rs/crates/worker-executor/src/control_api.rs#L348)、[dashboard:460–621](../hivemind-rs/crates/worker-executor/src/control_api.rs#L460)、[register／callback](../hivemind-rs/crates/worker-executor/src/control_api.rs#L973) |
| Next BFF／insecure server transport | [route.ts:23–73](../frontend/src/app/api/%5B...path%5D/route.ts#L23)、[HTTP handlers:129–205](../frontend/src/app/api/%5B...path%5D/route.ts#L129) |
| Rust Website routes／handlers | [routes:14–40](../hivemind-rs/crates/website-api/src/routes.rs#L14)、[handlers:132–463](../hivemind-rs/crates/website-api/src/handlers.rs#L132) |
| Enrollment TTL／single use | [auth enrollment](../hivemind-rs/crates/auth/src/enrollment.rs#L8)、[redemption](../hivemind-rs/crates/auth/src/enrollment.rs#L151) |
| Client VPN 與 identity enrollment | [VPN handoff](../hivemind-rs/crates/client-runtime/src/lib.rs#L2535)、[identity issue／redeem](../hivemind-rs/crates/client-runtime/src/lib.rs#L2460)、[Worker readiness](../hivemind-rs/crates/worker-executor/src/control_api.rs#L991)、[public-dynamic identity](../hivemind-rs/crates/worker-executor/src/control_api.rs#L1024) |
| JWT／refresh semantics | [jwt_service](../hivemind-rs/crates/auth/src/jwt_service.rs#L18)、[RefreshToken implementation](../hivemind-rs/crates/node-manager/src/grpc.rs#L691) |
| Browser token storage | [site store](../frontend/src/store/app-store.ts#L3)、[Master authSession](../frontend/master-ui/src/authSession.mjs)、[Worker authSession](../frontend/worker-ui/src/authSession.mjs) |
| UI workflow／polling | [Master App](../frontend/master-ui/src/App.jsx#L322)、[Worker App](../frontend/worker-ui/src/App.jsx#L221)、[Master observability](../frontend/master-ui/src/taskObservability.mjs#L16) |
| Same-origin Windows packages | [Master package:45–55](../scripts/package-master-windows.ps1#L45)、[Worker package:132–144](../scripts/package-worker-windows.ps1#L132) |
| Canonical protobuf／generation | [hivemind.proto](../proto/hivemind.proto#L11)、[vpn.proto](../proto/vpn.proto#L11)、[build.rs](../hivemind-rs/crates/proto/build.rs#L1) |
| General-compute request／gates | [runtime request／validation](../executor-rs/crates/general-compute-runtime/src/lib.rs#L111)、[Worker provider selection](../hivemind-rs/crates/worker-executor/src/executor.rs#L1252)、[production registration](../executor-rs/crates/general-compute-runtime/src/production.rs#L139)、[Windows HCS](../executor-rs/crates/general-compute-runtime/src/windows_hcs.rs#L69) |
| Managed GPU 執行與獨立結算 | [Worker GPU route](../hivemind-rs/crates/worker-executor/src/executor.rs#L812)、[GPU semantics](../executor-rs/crates/managed-function-runtime/managed-function-gpu-v1-semantics.json)、[完成派送分支](../hivemind-rs/crates/task-scheduler/src/dispatcher.rs#L3418)、[結果驗證與結算](../hivemind-rs/crates/task-scheduler/src/task_repository.rs#L6294) |
| Documentation contract | [release-docs checks](../scripts/release-docs.Tests.ps1#L34)、[site contract](../frontend/site-contract.test.mjs#L282) |

延伸閱讀：

- [文檔索引](README.md)
- [Getting Started](GETTING_STARTED.md)
- [Architecture](ARCHITECTURE.md)
- [Managed Function Runtime](MANAGED_FUNCTION_RUNTIME.md)
- [Managed consensus](managed-consensus-state.md)
- [Public Network Limitations](PUBLIC_NETWORK_LIMITATIONS.md)
- [Platform validation guide](platform-validation-state.md)
- [General compute OCI fixture](general-compute-oci-fixture.md)
- [ONNX guide](GENERAL_COMPUTE_ONNX.md)
- [Native UI／lifecycle checks](SHADCN_UI.md)
- [Smoke benchmark procedure](SMOKE_BENCHMARKS.md)
- [貢獻指南](../CONTRIBUTING.md)

產品目標與演進計畫是設計資料，不是現行支援矩陣：

- [零設定／跨平台目標](PRODUCT_GOAL_ZERO_CONFIG_CROSS_PLATFORM.md)
- [Runtime 演進計畫](MANAGED_RUNTIME_EVOLUTION_PLAN.md)
- [2026-06-26 歷史效能評估](UTILITY_PERFORMANCE_EVALUATION.md)

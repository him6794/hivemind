# HiveMind — Agent 開發規範

> 本文件為 AI Agent 實作 HiveMind 各模組時必須遵守的原則。
> 所有產出的程式碼應以**模組化、高擴展性、安全**為核心目標。

---

## 一、通用原則

### 1.1 模組化
- 每個功能單元必須是**獨立可替換**的模組，透過介面（interface / trait）而非具體實作互動。
- 禁止跨模組直接存取內部狀態；一律透過明確定義的公開 API。
- 單一職責原則（SRP）：每個檔案/結構體/函式只做一件事。
- 目錄即模組邊界：`pkg/<module>/` 內的程式不得 import 同層其他模組的內部型別，只能依賴共享的 `models/` 或 `proto` 定義。

### 1.2 高擴展性
- 新增功能**不得修改現有介面定義**，只能以新增方式擴充（Open/Closed Principle）。
- 所有策略性邏輯（排程演算法、計費公式、Executor 類型）以**策略模式（Strategy Pattern）** 封裝，可在 config 中切換，不需改程式碼。
- 設定值一律從**環境變數或設定檔**讀取，不得 hardcode 在程式碼中。
- 預留版本化路由（如 gRPC 的 `v1`、REST 的 `/api/v1/`），未來升級可並行運行舊版。

### 1.3 安全
- 所有對外 API 必須**先驗證再執行**，驗證失敗立即返回錯誤，不繼續處理。
- 敏感資料（密碼、Token、私鑰）不得出現在日誌、錯誤訊息或 struct 的 `String()` 輸出中。
- 任何來自外部的輸入（使用者、Worker 回報、任務包內容）視為不可信，必須驗證型別、範圍與格式。

---

## 二、各語言規範

### 2.1 Go（Master / NodePool / Worker）

#### 結構
```
pkg/
  <module>/
    <module>.go        # 對外公開介面定義（interface）
    impl.go            # 介面的預設實作
    <module>_test.go   # 單元測試
```

#### 命名
- 公開函式、型別：`PascalCase`
- 私有變數、函式：`camelCase`
- 常數：`PascalCase`（不使用全大寫 SCREAMING_SNAKE）
- 錯誤型別以 `Err` 為前綴：`ErrWorkerTimeout`

#### 錯誤處理
- 所有錯誤**必須被處理或明確向上傳遞**，禁止 `_ = someFunc()` 丟棄錯誤。
- 使用 `fmt.Errorf("context: %w", err)` 包裝錯誤，保留 stack chain。
- gRPC handler 中的錯誤統一使用 `status.Errorf(codes.XXX, ...)` 回傳，不直接暴露內部錯誤訊息。

#### 並發
- 共享狀態必須以 `sync.Mutex` 或 channel 保護，不得裸存取 map/slice。
- goroutine 必須有明確的生命週期管理（`context.Context` 取消或 `WaitGroup`）。
- 禁止無限制啟動 goroutine；需要池化的場景使用 worker pool 模式。

#### 安全
- 所有 gRPC endpoint 必須透過 middleware 驗證 JWT，只有 `Register` 與 `Login` 例外。
- 資料庫查詢一律使用**參數化查詢**（prepared statement），禁止字串拼接 SQL。
- 敏感環境變數（DB 密碼、JWT secret）讀取後立即儲存至不可導出欄位，不傳入日誌。

---

### 2.2 Rust（Executor）

#### 結構
```
executor-rs/src/
  lib.rs              # 對外公開 API（pub fn, pub trait）
  executor/
    mod.rs            # Executor trait 定義
    monty.rs          # Monty 實作
    <future>.rs       # 未來其他執行器實作
  resource/
    mod.rs            # 資源限制 trait
    limits.rs         # cgroup / rlimit 實作
  gpu/
    mod.rs            # GPU trait 定義（統一介面）
    nvidia.rs         # NVIDIA 實作
```

#### 規則
- 所有沙箱邊界跨越（FFI、syscall）必須封裝在 `unsafe` 區塊，並在同一函式內附上 `// SAFETY:` 說明。
- `Executor` trait 必須是**物件安全（object-safe）**，以支援動態分派。
- 資源限制（CPU time、記憶體）必須在任務啟動前設定完畢，不可在執行中途動態放寬。
- 任務執行在獨立 process 中，不與 Worker 主程式共享記憶體。

#### 安全
- 任務 zip 包解壓前必須驗證：路徑不得包含 `..`（防 path traversal）、每個檔案大小與總大小均有上限。
- 任何來自 Python 任務的輸出（stdout/stderr）必須限制最大位元組數，防止 OOM。
- 禁止在 Executor 中使用 `std::process::Command` 執行外部程式（除白名單例外）。

---

### 2.3 React（前端）

#### 結構
```
frontend/src/
  components/          # 純 UI 元件（無業務邏輯）
  features/            # 功能模組（含狀態管理與 API 呼叫）
    tasks/
    workers/
    account/
  api/                 # API client 封裝（統一 base URL、錯誤處理、token attach）
  hooks/               # 共用 custom hooks
  types/               # TypeScript 型別定義（對應 proto 定義）
```

#### 規則
- 使用 TypeScript，禁止使用 `any`；API response 型別必須明確定義。
- API 呼叫統一集中在 `api/` 層，元件內禁止直接呼叫 `fetch`/`axios`。
- 所有使用者輸入在送出前必須客戶端驗證，並在 Server 端再次驗證（不依賴前端驗證）。

#### 安全
- 禁止使用 `dangerouslySetInnerHTML`，如需渲染 HTML 必須先通過 DOMPurify 過濾。
- JWT Token 儲存於 `httpOnly cookie`（非 `localStorage`），防止 XSS 竊取。
- 所有 API 請求附帶 CSRF Token（若使用 cookie-based auth）。

---

## 三、跨服務規範

### 3.1 gRPC / Proto

- 所有訊息欄位必須加上 `validate` 標注（使用 `buf validate` 或 `protoc-gen-validate`）。
- 每次修改 `.proto` 必須保持**向下相容**：只新增欄位，不刪除、不改欄位序號。
- gRPC metadata 中的 Token 以 `authorization: Bearer <token>` 傳遞，不放在訊息 body。

### 3.2 錯誤回應格式

所有服務的錯誤統一以下結構回傳（避免洩漏內部細節）：

```
// gRPC
status.Errorf(codes.InvalidArgument, "invalid task format")    // ✅
status.Errorf(codes.Internal, err.Error())                     // ❌ 不可暴露 err 原文
```

- `codes.Internal` 只記錄在 server 端日誌，client 端只收到「internal error」。
- 業務邏輯錯誤使用具體的 gRPC status code：`NotFound`、`InvalidArgument`、`PermissionDenied` 等。

### 3.3 日誌規範

- 使用結構化日誌（Go: `slog`；Rust: `tracing`），禁止 `fmt.Println` 或 `println!`。
- 每條日誌必須包含：`service`、`request_id`、`level`。
- 日誌等級：
  - `DEBUG`：開發期除錯，生產環境預設關閉。
  - `INFO`：正常業務事件（任務分配、節點上線）。
  - `WARN`：可恢復的異常（重試、超時）。
  - `ERROR`：需要人工介入的錯誤。
- **禁止**在任何等級的日誌記錄：密碼、Token、CPT 帳戶私鑰、使用者個資。

### 3.4 設定管理

- 所有可設定項目必須在 `config/config.go`（或 `config.rs`）中集中定義，附上預設值與說明。
- 設定的讀取順序：環境變數 > 設定檔 > 預設值。
- 啟動時列印目前有效設定（隱藏敏感值），方便 debug。

```go
// 範例
type Config struct {
    GRPCPort       int    `env:"GRPC_PORT" default:"50051"`
    DBConnStr      string `env:"DB_CONN" secret:"true"`  // 日誌中遮蔽
    HeartbeatSec   int    `env:"HEARTBEAT_SEC" default:"30"`
}
```

---

## 四、測試規範

### 4.1 測試撰寫原則
- 每個模組的核心邏輯（排程、計費、資源限制）必須有**單元測試**，覆蓋率目標 ≥ 80%。
- gRPC handler 以**整合測試**搭配 mock storage 測試。
- 測試檔案與源碼同目錄，命名為 `<file>_test.go` 或 `<file>_test.rs`。
- 禁止在測試中使用真實資料庫或網路；使用 mock / in-memory 替代。
- 表驅動測試（table-driven tests）優先，方便一次覆蓋多個邊界情況。
- 每個測試案例必須涵蓋：正常路徑（happy path）、邊界值、錯誤路徑至少各一個。

### 4.2 測試執行指令

每次完成實作後，必須執行對應的測試指令並確認全部通過才算完成：

| 服務 | 指令 | 說明 |
|------|------|------|
| Go 服務（nodepool/master/worker） | `go test ./...` | 在各服務目錄下執行 |
| Go 加覆蓋率報告 | `go test ./... -cover` | 需達到 ≥ 80% |
| Rust executor | `cargo test` | 在 `executor-rs/` 目錄下執行 |
| React 前端 | `npm test -- --watchAll=false` | 在 `frontend/` 目錄下執行 |

### 4.3 測試失敗的處理
- 測試失敗時，必須**先修復**再繼續下一個任務，不得跳過。
- 禁止使用 `t.Skip()`、`#[ignore]` 或 `xit()` 暫時略過測試，除非附上明確的 TODO 說明與 issue 追蹤。
- 若測試因外部依賴失敗（如網路、DB），檢查 mock 是否正確設定，不可因此刪除測試。

---

## 五、擴充點清單（Extension Points）

以下是系統中預留的擴充點，實作時必須以介面定義，不得寫死：

| 擴充點 | 介面名稱 | 說明 |
|--------|---------|------|
| 任務排程演算法 | `Scheduler` | 預設：資源分數最高優先；未來可換成競價制 |
| Python 執行器 | `Executor` | 預設：Monty；未來可加 Docker、WASM |
| GPU 後端 | `GPUBackend` | 預設：NVIDIA CUDA；未來加 AMD ROCm |
| 任務分發通道 | `Distributor` | 預設：BitTorrent；備案：Object Storage |
| CPT 計費公式 | `BillingPolicy` | 費率規格變更不需改核心邏輯 |
| 帳號驗證方式 | `Authenticator` | 預設：JWT；未來可加 OAuth 或 API Key |
| 通知管道 | `Notifier` | 預設：gRPC stream；未來可加 email、webhook |

---

## 六、禁止事項（Forbidden Patterns）

以下行為在任何情況下禁止，Agent 輸出的程式碼不得包含：

- ❌ Hardcode 任何 IP、Port、密碼、Token、Secret
- ❌ 在 gRPC/REST response 中暴露 stack trace 或內部錯誤原文
- ❌ SQL 字串拼接（必須使用參數化查詢）
- ❌ 直接在 goroutine 中操作共享 map 而不加鎖
- ❌ 任務 zip 解壓時不驗證 path（防 zip slip / path traversal）
- ❌ 在 Executor 中以 root 執行任務（必須降權）
- ❌ 跳過 JWT 驗證的 workaround（如 `if dev { skip auth }`）
- ❌ 前端以 `localStorage` 儲存 JWT
- ❌ 在日誌中記錄完整 JWT 或密碼
- ❌ 無錯誤處理的 goroutine（`go func()` 內 panic 不能靜默吞掉）
- ❌ 在測試未通過前執行 `git commit`

---

## 七、Git 使用規範

### 7.1 分支策略

```
main          ← 永遠保持可運行狀態，只接受來自 dev 的 merge
dev           ← 日常開發主線
feat/<name>   ← 新功能分支，從 dev 切出
fix/<name>    ← 修復分支，從 dev 切出
```

- 所有開發在功能分支進行，**禁止直接 commit 到 main**。
- 分支命名使用 kebab-case：`feat/nodepool-scheduler`、`fix/jwt-expiry`。

### 7.2 Commit 規範

格式遵循 [Conventional Commits](https://www.conventionalcommits.org/)：

```
<type>(<scope>): <簡短描述>

[選填：詳細說明]
```

| type | 用途 |
|------|------|
| `feat` | 新增功能 |
| `fix` | 修復 bug |
| `test` | 新增或修改測試 |
| `refactor` | 重構（不改行為） |
| `chore` | 設定、依賴更新 |
| `docs` | 文件變更 |

scope 使用服務或模組名稱：`nodepool`、`worker`、`executor`、`frontend`、`proto`

**範例：**
```
feat(nodepool): implement resource-score-based scheduler
fix(worker): handle heartbeat timeout edge case
test(nodepool): add table-driven tests for billing policy
```

### 7.3 Agent 每次任務的強制工作流程

Agent 完成一個任務單元後，**必須依序執行以下步驟，全部通過才算完成**：

```
1. 撰寫 / 更新程式碼
        ↓
2. 撰寫對應的單元測試（若尚未存在）
        ↓
3. 執行測試，確認全部通過
   go test ./...          （Go 服務）
   cargo test             （Rust executor）
   npm test -- --watchAll=false  （React 前端）
        ↓
4. 確認無編譯錯誤 / lint 警告
   go vet ./...           （Go）
   cargo clippy           （Rust）
        ↓
5. git add（只加與本次任務相關的檔案）
        ↓
6. git commit（遵循 Conventional Commits 格式）
        ↓
7. 若功能完整，merge 回 dev 分支
```

> ⛔ 嚴禁在測試失敗或有編譯錯誤的情況下執行 `git commit`。
> ⛔ 嚴禁使用 `git commit --no-verify` 跳過 hook 檢查。
> ⛔ 嚴禁一次 commit 包含多個不相關的功能變更（atomic commit 原則）。

### 7.4 .gitignore 規範

以下內容**必須**加入 `.gitignore`，禁止提交：

```
# 環境與密鑰
.env
*.env.local
*.pem
*.key

# 編譯產物
/target/          # Rust
/dist/            # 前端
*.exe

# 依賴
/node_modules/
/vendor/          # Go（若使用 vendor mode）

# 測試覆蓋率報告
coverage.out
coverage.html
```

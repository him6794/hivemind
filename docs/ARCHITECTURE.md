# Hivemind 平台架構

Hivemind 是以 Rust 為核心的分散式運算平台，主要由
`hivemind-rs` workspace 與 `hivemind-bin` runtime entry point 組成。

本平台對外採用**登入後即可加入的開放式模型**：任何能透過 Website API
登入的使用者，都可以註冊 Master 或 Worker 並加入 Headscale overlay。
這裡的「開放加入」不代表匿名存取，也不代表跳過 Nodepool 的伺服器端
授權、能力檢查、配額、聲譽、consensus 驗證或結算檢查。

## 最終產品目標：零設定使用體驗

平台的最終使用門檻應該盡可能接近「下載、登入、放著就能用」。使用者
不應該需要理解或手動操作 Headscale、Nodepool、Worker execution、Worker ID、
lease、billing 或 settlement。

### Worker 使用者

Worker 使用者只需要：

```text
1. 下載對應作業系統的 Worker client。
2. 啟動 client 並登入 Website API。
3. 讓 client 持續執行。
```

登入後，client 應自動完成：

- 取得短期 enrollment credential。
- 取得一次性的 Headscale enrollment material。
- 建立或恢復本機 overlay identity。
- 向 Nodepool 註冊 Worker 與 authenticated owner。
- 自動取得可用的 closed-DSL runtime 與平台政策。
- 執行 capability/readiness handshake。
- 在背景接收符合條件的任務並回報結果；consensus task 不需額外本機服務。

Worker 使用者不應該手動填寫 local executable path、port、Worker ID、owner、trust list
或 Worker execution certificate path。這些資料由 Website API、Headscale 與 Nodepool 的
enrollment/provisioning 流程自動處理。需要 operator policy 的項目應由平台服務端管理，
不應成為一般使用者的部署工作。

### Master 使用者

Master 使用者只需要：

```text
1. 登入 Master client 或 Web UI。
2. 上傳已寫好的封閉 DSL 程式與 input。
3. 帳戶保有足夠餘額。
4. 等待結果。
```

Master 使用者不需要：

- 選擇某一台 Worker。
- 設定 Headscale 或 Nodepool 位址。
- 管理 lease 或 retry。
- 手動驗證結果。
- 計算 usage、billing 或 settlement。
- 了解任務實際在哪一台主機執行。

平台應自動負責 quote/preflight、Worker 選擇、排程、DSL 執行、consensus
round/quorum、重試、usage accounting、billing、settlement 與結果回傳。
Managed task 只有在多個 Worker 回報一致結果並達到 quorum 後才會結算；無法取得
quorum 時，任務保持未結算或失敗。使用者只需要取得成功結果或清楚的失敗原因。

### 平台內部與使用者體驗的分離

```text
使用者看到：
  登入 -> 上傳程式 -> 等待 -> 取得結果

平台內部處理：
  enrollment -> Headscale -> Nodepool admission -> quote -> lease
  -> Worker execution -> consensus replicas -> quorum certificate
  -> usage -> fixed-reservation billing -> settlement -> result/log retrieval
```

所有複雜的 trust、network、consensus 與 settlement 流程都必須留在平台內部，
不能要求一般使用者以手動設定的方式參與。若某個流程需要使用者手動複製
憑證、填寫 Worker ID、編輯 trust list 或啟動額外本機服務，代表該流程尚未達到
最終產品目標。

## Workspace 結構

```text
hivemind-rs/
  crates/common           共用 tracing、錯誤與輔助工具
  crates/config           環境變數與檔案設定
  crates/proto            產生的 gRPC contract
  crates/models           共用 domain type
  crates/database         PostgreSQL 與 Redis 存取
  crates/auth             註冊、登入與 token 處理
  crates/node-manager     Worker 註冊、heartbeat、admission、清理
  crates/task-scheduler   派送、重新派送、lease 與 timeout
  crates/master-api       HTTP API 與 proxy layer
  crates/worker-executor  managed-function 執行與 Worker control API
  crates/vpn-service      Headscale/VPN peer 管理
  crates/hivemind-bin     runtime entry point
```

Repository 另外包含：

- Official Site：`frontend/`
- Master UI：`frontend/master-ui`
- Worker UI：`frontend/worker-ui`

## 平台架構

```text
                         公開使用者／瀏覽器邊界
                                      |
                              登入、帳號、註冊
                                      v
                          +--------------------------+
                          | Website API               |
                          | 帳號與 enrollment         |
                          +-------------+------------+
                                        |
                    一次性、角色限定的 enrollment credential
                                        v
                          +--------------------------+
                          | Headscale / VPN overlay   |
                          | peer identity 與 routing  |
                          +-------------+------------+
                                        |
                    只允許經過驗證的 overlay connection
                                        v
+-------------------+       +--------------------------+       +-------------------+
| Master            |------>| Nodepool                 |<------| Worker            |
| 使用者任務 client | gRPC  | identity、排程、lease、  | gRPC  | 使用者部署的     |
| 在其他主機執行    |       | consensus、usage、      |       | closed DSL runtime|
+-------------------+       | settlement、audit       |       +-------------------+
                            +------------+-------------+
                                         |
                                         | PostgreSQL + Redis
                                         v
                            +--------------------------+
                            | PostgreSQL / Redis       |
                            | 平台權威狀態             |
                            +--------------------------+
```

Nodepool 是 settlement authority。Consensus-enabled managed tasks create a durable
round and slot reservations, assign the same deterministic side-effect-free task to
distinct Workers, compare canonical result bytes, and accept only a strict majority
certificate. Worker 回報只是待驗證的 observation。

```text
Nodepool
  -> replica-1 / Worker A (attempt-bound token)
  -> replica-2 / Worker B (attempt-bound token)
  -> replica-3 / Worker C (attempt-bound token)
  -> persisted observations -> canonical quorum certificate
  -> atomic task completion + fixed-reservation settlement
```

Worker 回報的結果與 usage 都只是 claim；Nodepool 必須獨立完成 quorum、result
binding、billing 與 settlement。Consensus 是 authenticated Worker agreement，
可降低單一 Worker 錯誤的影響，但 colluding Workers 或 common-mode runtime bugs
仍可能產生一致但錯誤的結果。

## 部署拓撲

### Orange Pi control plane

正式 Orange Pi 部署只包含：

```text
Orange Pi ARM64
  - Nodepool
  - Website API
  - Headscale
  - PostgreSQL
  - Redis
```

Orange Pi 不得執行 Master 或 Worker。Master 與 Worker 都必須在 Orange Pi
之外執行；Linux-only infrastructure stays in the control plane or local Docker
during development and is never a Worker prerequisite.

root all-in-one Compose stack 只供本機開發與 release smoke test 使用，
不是正式外部拓撲，也不能作為外部 Headscale 已連通的證據。

### 外部運算主機

```text
適合的外部主機
  - Master UI/API 或 CLI
  - native Windows/Linux Worker
  - local Docker services needed for Linux-only infrastructure
```

Native Windows Worker 會在本機執行封閉的 managed DSL。Consensus-enabled
managed tasks 只需要已打包的 closed-DSL runtime，並由 Nodepool 將同一個
deterministic task 派送給多個 Worker；不要求使用者安裝 Docker、WSL、Rust、
Cargo 或其他 runtime。Worker readiness、replica 數量或 quorum 不足時，
任務保持未結算或失敗，不會改走單一 Worker 路徑。

## 公開註冊與開放式加入

正常的公開 onboarding 不需要使用者手動維護 Worker allowlist。流程如下：

```text
1. 使用者透過 Website API 建立帳號或登入。
2. Website API 回傳短期、角色限定的 enrollment credential。
3. Master/Worker 使用 credential 申請一次性的 VPN enrollment。
4. Worker 加入 Headscale，取得 overlay identity。
5. Worker 透過已驗證的 overlay 向 Nodepool 註冊。
6. Nodepool 將 Worker identity 綁定到已驗證的 owner。
7. Nodepool 記錄並驗證 Worker 的 capability/readiness 狀態。
8. Worker 依照相容性與平台政策進入可排程狀態。
```

使用者不應該需要手動編輯：

```text
HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES
```

也不應該需要把 Worker ID 複製到 operator 維護的 allowlist。
公開網路的 admission 是動態且由伺服器端執行：Nodepool 驗證 identity、
liveness、capability 相容性、資源政策、配額、聲譽與 readiness。
不符合政策的 Worker 不會被派發該任務，但仍然可以加入網路，不需要先
人工批准才能註冊。

「開放加入」仍然可以包含反濫用措施。Nodepool 可以使用帳號限制、rate
limit、聲譽、stake/bond、容量限制、lease 檢查與失敗懲罰。這些是平台
政策與帳務控制，不是逐台 Worker 的人工 cryptographic allowlist。

### 目前實作差距

目前公開 onboarding 與自動 enrollment 仍需要正式 Website API、Headscale
與 Nodepool 部署。Nodepool 負責選擇具備 readiness 的 Worker、驗證 execution
identity、建立 consensus round，並在 quorum 不足時保持任務未結算。

Windows Worker 的登入、註冊與本機 closed-DSL 啟動已有零設定 package；完整的
外部 VPN、三個 eligible Worker 與多 Worker settlement 仍需要正式環境驗證。

## Enrollment 與 VPN 邊界

互動式 enrollment 必須將 `WEBSITE_API_BASE`，或角色專用的
`MASTER_WEBSITE_API_BASE` / `WORKER_WEBSITE_API_BASE`，設定為正式部署的
Rust Website API HTTPS origin。該 API 必須提供：

```text
POST /api/login
POST /api/vpn/config
```

其中 `/api/vpn/config` 必須是受保護的 authenticated route；不能假設官方 Next BFF
本身就是 VPN-config service。

本機 UI 或 launcher 會將使用者的 bearer JWT 傳給 Website API。一次性的
Headscale key 只在 process memory 中使用。以下資料不得回傳到 browser
storage 或放入 Worker package：

- password
- reusable Headscale key
- raw one-time key
- `HEADSCALE_API_KEY`

Operator 也可以為 unattended startup 配置角色限定的
`MASTER_VPN_AUTHKEY` 或 `WORKER_VPN_AUTHKEY`。這是啟動時的操作憑證，不是
公開 Worker trust-list entry。互動式與 operator-provisioned startup 都必須
等待真正的 Nodepool transport handshake，並在 bounded timeout 內失敗時
停止啟用註冊或 task operation。

重啟時會先嘗試使用已保存的 libtailscale state。互動式 JWT 過期後，必須
重新登入。自動更新與自動下載目前不在範圍內。

## 完整平台流程

### 1. 帳號與 enrollment

1. 使用者透過 Website API 登入。
2. Website API 驗證使用者並簽發短期、角色限定的 enrollment credential。
3. 本機 Master 或 Worker 使用 credential 取得一次性的 Headscale enrollment
   material。
4. Headscale 配置 overlay peer identity 與 route。
5. runtime 透過 overlay 探測 Nodepool 並註冊 identity。
6. Nodepool 將 Worker 綁定到登入的 owner，並記錄 liveness/capability。
   公開流程不需要另外手動註冊 Worker。

### 2. Quote 與排程

1. 使用者透過 Master UI、Master API 或 CLI 提交任務。
2. Nodepool 驗證帳號授權、任務限制、DSL runtime 與資源需求。
3. Nodepool 按照目前價格與政策回傳 resource quote。
4. 使用者接受 quote 後，scheduler 按照資源、註冊 capability、Worker
   readiness、quota、reputation 與政策，選出可用 Worker。
5. Nodepool 建立 active lease，並產生穩定的 execution identity、attempt
   identity 與 lease generation。

### 3. Worker 執行

1. Worker 接收任務並驗證有界的 DSL source/input。
2. Worker 在本機的 closed managed DSL runtime 執行任務，不能透過 managed
   runtime 執行任意 host command。
3. Nodepool 將同一個 deterministic、side-effect-free task 派送給不同 Worker，
   每個 Worker 都使用 attempt-bound token 回報 canonical result。
4. Worker 無法取得有效的 assignment 或 Nodepool readiness 時，不會自行切換到
   未信任的 local 或 direct path。

### 4. Consensus replica 執行

1. Nodepool 建立 durable consensus round 與 replica slot reservations。
2. 三個不同 Worker 執行同一個固定 task；Nodepool 驗證每個 response 的 identity、
   attempt、result digest、output bytes 與 protocol bounds。
3. Nodepool 持久化 observations，將相同 canonical result 分組，並只接受 strict
   majority quorum certificate。
4. Worker 數量不足、結果不一致、round 過期或 quorum 無法完成時，任務保持未結算
   或失敗，不會改走單一 Worker completion。

### 5. 驗證、結算與結果取得

1. Nodepool 驗證 task、Worker、attempt、policy、result digest、output、budget 與
   managed DSL backend/semantics binding。
2. Quorum certificate 與相符的 replica evidence 必須一併持久化；沒有 certificate
   就不能標記完成、寫入 output、billing 或 settlement。
3. Billing 使用 Nodepool-owned fixed reservation，不採用 Worker 自己回報的價格或
   usage 作為結算依據。
4. 已授權的使用者透過 Website/Master API 取得 result、logs 與 lifecycle evidence。
   Secrets 與內部 enrollment material 不會回傳。

## 主要服務角色

### Official Site 與 Website API

- 提供公開產品內容與登入後的帳號中心。
- 負責帳號註冊、登入、餘額顯示與 enrollment handoff。
- Rust Website API 提供受保護的 VPN enrollment route。
- 不提交任務、不執行程式、不執行 settlement。
- Browser 不直接連線 Nodepool gRPC。

### Headscale / VPN Service

- 提供已驗證的 overlay identity 與 routing boundary。
- 透過受保護的 Website API 流程發出或使用 enrollment material。
- 不負責帳戶餘額、任務狀態或 billing。

### Master UI/API

- 提供任務操作者使用的應用程式或 CLI-facing API。
- 驗證使用者，並將任務提交、quote、狀態、取消、logs、results 與 artifact
  操作 proxy 到 Nodepool。
- 在正式拓撲中執行於 Orange Pi control plane 之外。

### Node Manager

- 註冊 Worker，追蹤 heartbeat、liveness、identity 與 dynamic admission state。
- 將 Worker 綁定到 enrollment 時驗證的 owner。
- 不接受 Worker 自己聲稱的 capability 作為未驗證的信任證據。

### Task Scheduler

- 從 pending work 中選擇符合條件的 Worker。
- 建立並驗證 task lease、attempt identity 與 consensus replica slots。
- 處理 redispatch、timeout、stale lease 與 retry policy，但不能把 settlement
  權限交給 Worker。

### Worker Executor

- 在 closed managed-function runtime 中執行 active `managed-function-v1` 任務。
- 追蹤本機資源使用量，並提供 Worker gRPC/control HTTP endpoints。
- 只回報帶有 assignment identity 的 result observation；不擁有 settlement 權限。
- 保留解析既有 v0 執行與歷史 evidence 的相容邊界，但不接受新的 v0 work。

## Trust 與資料邊界

Nodepool 是以下事項的唯一平台 authority：

```text
account execution authorization
Worker identity binding
lease、scheduling 與 replica reservations
consensus policy and certificate binding
result and output validation
verified usage
billing 與 settlement
audit lifecycle
```

Master 與 Worker 都是 untrusted caller 或 execution host，其 claim 必須由 Nodepool
驗證。PostgreSQL 與 Redis 保存平台狀態，只能由 Nodepool 後端使用，
不得暴露給 Worker、browser 或下載的 package。

### Credential 放置位置

```text
Website API / Headscale control plane
  - account/session credential
  - Headscale control secret

Nodepool only
  - database 與 Redis credential
  - Worker execution signing key
  - settlement authority

Worker
  - Worker execution verification key
  - no Nodepool private key or database credential

Worker package
  - no Nodepool database credential
  - no HEADSCALE_API_KEY
  - no reusable bearer token
```

Worker execution tokens authenticate the assigned task attempt. They do not turn the
Worker into a settlement authority; only Nodepool can verify a quorum certificate and settle.

## 失敗與安全規則

- enrollment credential 缺少或過期時，enrollment fail closed。
- Headscale 或 Nodepool readiness 失敗時，不得啟用 registration 或 task operation。
- stale lease、request digest 改變、Worker/owner 錯誤、attempt 錯誤或 malformed
  response 時，不得產生可 billing 的完成結果。
- consensus policy 缺少、replica 不足、結果不一致、certificate 無效或 replica
  evidence 不完整時，managed task 必須保持未結算或失敗。
- generic single-Worker completion、observe evidence 與 Worker 自己的 usage claim
  都不能授權 managed settlement。
- source、input、password、raw JWT、Headscale key 與 private key 不得寫入一般
  log 或 durable state。
- 不得因 consensus 暫時不可用而切換到未驗證或單一 Worker 路徑。

## 現有 Contract

- `proto/hivemind.proto` 定義 Worker/Master/Nodepool 共用的 gRPC surface。
- `managed-consensus` 定義 consensus binding、observation、quorum policy 與
  certificate contract。
- 新任務只使用 `managed-function-v1`：它沿用封閉的 source function/JSON input
  interpreter，並以每個 replica 的實際執行 operation 做 Nodepool settlement。
  `managed-function-v0` 僅保留歷史相容契約；舊資料可讀取，但新的 v0 submission
  會被拒絕，未完成的 v0 work 會在 Nodepool 啟動時取消。
- Managed function 不執行使用者提供的 executable，也不需要 HCS；只有
  `general-compute-v1alpha1` 的 arbitrary-compute 路徑使用原生 Windows HCS。
- `hivemind-bin` 可以在本機開發時執行 `master`、`nodepool`、`worker` 或
  `all`；正式 Orange Pi 部署使用 role-specific services。
- binary 也提供 `submit`、`status` 與 `result` CLI helper。

## 預設位址

- Official Site：`0.0.0.0:8080`（Compose host mapping）
- Master UI：`0.0.0.0:3000`（Compose host mapping）
- Worker UI：`0.0.0.0:3001`（Compose host mapping）
- Nodepool gRPC：`0.0.0.0:50051`
- Master HTTP：`0.0.0.0:8082`
- Worker gRPC：`0.0.0.0:50053`
- Worker control HTTP：`127.0.0.1:18080`
- Managed consensus：由 Nodepool 建立 replica round，不需要額外的本機服務。

以上是 development 或 service default，不是 live credential。
實際部署可以透過 environment variable 或 JSON config file 覆寫。

## 目前狀態

- Rust workspace 是 authoritative implementation。
- `docs_backup_20260611_202024/` 中較舊的 Python-era architecture notes
  只作為歷史參考。
- 本文件描述的公開 enrollment、dynamic admission、versioned managed runtime
  與 consensus settlement model 是 authoritative product architecture；未配置
  的外部 credential 或 operator asset 仍必須 fail closed。
- 目前本機測試已涵蓋 policy admission、replica fan-out、quorum certificate、
  certificate-backed settlement、accepted-replica usage billing 與 fail-closed
  completion guard。
- 完整外部證據仍需要在 Orange Pi 之外完成 Website enrollment、Headscale overlay
  connectivity、至少三個 eligible Worker、Worker registration、result/log
  retrieval、usage、billing、settlement 與 audit evidence。
- Signed client update runtime 已實作 archive verification、activation 與
  rollback guard；在 production-signed keyset、核准 HTTPS endpoint、clean-host
  live update 與 minimum-supported-version evidence 齊備前，更新保持
  fail-closed/deferred，不使用 unsigned fallback。

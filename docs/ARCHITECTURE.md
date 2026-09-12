# Hivemind 平台架構

Hivemind 是以 Rust 為核心的分散式運算平台，主要由
`hivemind-rs` workspace 與 `hivemind-bin` runtime entry point 組成。

本平台對外採用**登入後即可加入的開放式模型**：任何能透過 Website API
登入的使用者，都可以註冊 Master 或 Worker 並加入 Headscale overlay。
這裡的「開放加入」不代表匿名存取，也不代表跳過 Nodepool 的伺服器端
授權、能力檢查、配額、聲譽、consensus/proof 驗證或結算檢查。

## 最終產品目標：零設定使用體驗

平台的最終使用門檻應該盡可能接近「下載、登入、放著就能用」。使用者
不應該需要理解或手動操作 Headscale、Nodepool、Worker sidecar、Worker execution、Worker ID、
proof token、lease、billing 或 settlement。

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
- 在背景接收符合條件的任務並回報結果；consensus task 不需額外 prover sidecar。

Worker 使用者不應該手動填寫 local executable path、port、Worker ID、owner、Worker sidecar
trust list、Worker execution certificate path 或 proof request binding。這些資料由 Website
API、Headscale 與 Nodepool 的 enrollment/provisioning 流程自動處理。需要
operator policy 的項目應由平台服務端管理，不應成為一般使用者的部署工作。

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
- 配置 managed-prover Worker sidecar。
- 管理 proof token、lease 或 retry。
- 手動驗證 receipt。
- 計算 usage、billing 或 settlement。
- 了解任務實際在哪一台主機執行。

平台應自動負責 quote/preflight、Worker 選擇、排程、DSL 執行、consensus
round/quorum、重試、usage accounting、billing、settlement 與結果回傳。
只有 legacy proof-backed attempt 才需要 local managed proof 與 receipt 驗證。
使用者只需要取得成功結果或清楚的失敗原因。

### 平台內部與使用者體驗的分離

```text
使用者看到：
  登入 -> 上傳程式 -> 等待 -> 取得結果

平台內部處理：
  enrollment -> Headscale -> Nodepool admission -> quote -> lease
  -> Worker execution -> consensus replicas -> quorum certificate
  -> usage -> fixed-reservation billing -> settlement -> result/log retrieval
  (legacy proof-backed attempts additionally use local managed proof and receipt verification)
```

所有複雜的 trust、network、proof 與 settlement 流程都必須留在平台內部，
不能要求一般使用者以手動設定的方式參與。若某個流程需要使用者手動複製
憑證、填寫 Worker ID、選擇 Worker sidecar 或編輯 trust list，代表該流程尚未達到
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
| 在其他主機執行    |       | consensus/proof、usage、 |       | closed DSL runtime|
+-------------------+       | settlement、audit       |       +---------+---------+
                            +------------+-------------+                 |
                                         |                                | 有界、
                                         | PostgreSQL + Redis             | task-scoped
                                         v                                | proof request
                            +--------------------------+                  v
                            | PostgreSQL / Redis        |       +-------------------+
                            | 平台權威狀態              |       | Managed-prover    |
                            +--------------------------+       | Worker sidecar          |
                                                               | 獨立的 Linux      |
                                                               | x86_64 operator   |
                                                               +---------+---------+
                                                                         |
                                                               pinned RISC Zero
                                                               prover sidecar
```

Worker sidecar 只是 legacy proof route 的執行基礎設施，不是 settlement
authority。Consensus-enabled managed tasks do not use that sidecar: Nodepool
creates a durable round and slot reservations, assigns the same deterministic
side-effect-free task to distinct Workers, compares canonical result bytes, and
accepts only a strict majority certificate.

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
不是 zkVM correctness proof；colluding Workers 或 common-mode runtime bugs
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

Orange Pi 不得執行：

```text
  - Master
  - Worker
  - managed-prover Worker sidecar
  - Linux RISC Zero proving sidecar
```

root all-in-one Compose stack 只供本機開發與 release smoke test 使用，
不是正式外部拓撲，也不能作為外部 Headscale 已連通的證據。

### 外部運算與 legacy proving 主機

Master 與 Worker 都必須在 Orange Pi 之外執行。只有仍服務既有
proof-backed attempts 的 managed-prover infrastructure 才需要額外的 Linux
主機：

```text
適合的外部主機
  - Master UI/API 或 CLI
  - native Windows/Linux Worker（consensus route 不需 Docker、WSL、Rust、Cargo 或 RISC Zero）
  - legacy proof tasks 需要的 Linux x86_64 managed-prover Worker sidecar
```

Native Windows Worker 會在本機執行封閉的 managed DSL，不會攜帶或執行
Linux RISC Zero prover。Consensus-enabled managed tasks 只需要已打包的
closed-DSL runtime，並由 Nodepool 透過多 Worker quorum 接受結果；不要求
使用者安裝 Docker、WSL、Rust、Cargo、RISC Zero 或其他 runtime。仍服務
legacy managed proof tasks 時，Worker 才使用明確配置且經過驗證的
Worker-local sidecar；sidecar 不存在、不可用、未通過驗證、能力不相容、
授權失敗或回傳無效 proof 時，該 legacy task 必須 fail closed。

## 公開註冊與開放式加入

正常的公開 onboarding 不需要使用者手動把 Worker 加入 Worker sidecar trust list。
流程如下：

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

也不應該需要把 Worker ID 複製到 operator 維護的 Worker sidecar allowlist。
公開網路的 admission 是動態且由伺服器端執行：Nodepool 驗證 identity、
liveness、capability 相容性、資源政策、配額、聲譽與 readiness。
不符合政策的 Worker 不會被派發該任務，但仍然可以加入網路，不需要先
人工批准才能註冊。

「開放加入」仍然可以包含反濫用措施。Nodepool 可以使用帳號限制、rate
limit、聲譽、stake/bond、容量限制、lease 檢查與失敗懲罰。這些是平台
政策與帳務控制，不是逐台 Worker 的人工 cryptographic allowlist。

### 目前實作差距

目前部署只保留 Worker 本機的 managed-proof sidecar。Nodepool 負責選擇具備
readiness 的 Worker、驗證 Worker execution token 與 proof envelope；不再建立
或傳遞獨立的 network proving grant、TLS client material 或 queue state。

Worker 的本機 sidecar 若缺少、失敗或回傳不符合 local JSON contract 的資料，任務
會 fail closed。公開 admission 仍由 Nodepool 的 capability、資源、配額與聲譽
政策控制，不需要另一個 proving service 的 allowlist。

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
storage、放入 Worker package 或送給 Worker sidecar：

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
   公開流程不需要 Worker sidecar 另外手動註冊 Worker。

### 2. Quote 與排程

1. 使用者透過 Master UI、Master API 或 CLI 提交任務。
2. Nodepool 驗證帳號授權、任務限制、DSL runtime 與資源需求。
3. Nodepool 按照目前價格與政策回傳 resource quote。
4. 使用者接受 quote 後，scheduler 按照資源、註冊 capability、Worker sidecar
   readiness、quota、reputation 與政策，選出可用 Worker。
5. Nodepool 建立 active lease，並產生穩定的 execution identity、attempt
   identity 與 lease generation。

### 3. Worker 執行

1. Worker 接收任務並驗證有界的 DSL source/input。
2. Worker 在本機的 closed managed DSL runtime 執行任務，不能透過 managed
   runtime 執行任意 host command。
3. 如果任務需要 managed proof，且 Worker 已配置 Worker-local sidecar，Worker
   送出有界且 canonical 的 proof request。
4. 如果沒有經過驗證的 Worker sidecar，managed execution fail closed；Worker
   不得偷偷切換到未信任的 local 或 direct path。

### 4. Local managed proof 與 Worker sidecar 執行

1. Worker 以受限 stdin/stdout JSON contract 將 task source、input 與 usage budget
   送給本機 `MANAGED_PROVER_EXECUTABLE`。
2. sidecar 使用固定的 proof host 與已核准的 guest image 產生 envelope；Worker
   以 bounded IO、semaphore、timeout、cancellation 與 child cleanup 保護邊界。
3. Worker 不把 source、input 或 proof payload 放入 command-line arguments、一般
   log 或 durable queue state。
4. sidecar 缺少、不可用、超時、取消、輸出超界或 JSON 不合法時，Worker 回傳
   generic failure，不會切換到 direct host execution 或未核准的遠端路徑。

### 5. 驗證、結算與結果取得

1. Worker 驗證有界的 Worker sidecar response，並透過正常任務流程回傳結果與
   proof metadata。
2. Nodepool 驗證 task/Worker/attempt identity、rollout policy、response
   bounds、proof scheme、精確的 pinned guest image、receipt shape、journal、
   cryptographic receipt、ExecutionClaim、source/input/output digest、budget
   與 managed DSL backend/semantics binding。
3. 所有 proof 與 identity gate 都通過後，Nodepool 才將任務標記為完成並
   記錄 verified usage。
4. Billing 與 settlement 由 Nodepool 根據 verified usage 與 idempotent
   task/attempt state 執行。Worker sidecar 的 claim 不是 settlement evidence。
5. 已授權的使用者透過 Website/Master API 取得 result、logs 與可供 audit
   的 lifecycle evidence。Secrets 與 raw proof envelope 不會回傳。

## 主要服務角色

### Official Site 與 Website API

- 提供公開產品內容與登入後的帳號中心。
- 負責帳號註冊、登入、餘額顯示與 enrollment handoff。
- Rust Website API 提供受保護的 VPN enrollment route。
- 不提交任務、不執行程式、不驗證 receipt、不執行 settlement。
- Browser 不直接連線 Nodepool gRPC。

### Headscale / VPN Service

- 提供已驗證的 overlay identity 與 routing boundary。
- 透過受保護的 Website API 流程發出或使用 enrollment material。
- 不負責帳戶餘額、任務狀態、proof 驗證或 billing。

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
- 建立並驗證 task lease 與 attempt identity。
- 處理 redispatch、timeout、stale lease 與 retry policy，但不能把 settlement
  權限交給 Worker sidecar。

### Worker Executor

- 在 closed managed-function runtime 中執行 `managed-function-v0` 任務。
- 追蹤本機資源使用量，並提供 Worker gRPC/control HTTP endpoints。
- 需要 managed proof 時，只能透過 authenticated bounded Worker sidecar protocol
  使用 local managed proof infrastructure。

### Managed-prover Worker sidecar

- 在 proof-capable Worker 上以本機 bounded child process 執行。
- 僅接受 retained `ManagedProverRequest` JSON，回傳 `ManagedProverResponse` JSON。
- 不提供網路服務、不保存 queue/job state、不接受遠端 grant，也不擁有 billing
  或 settlement 權限。
- Nodepool 仍獨立驗證 `ManagedProofEnvelope` 的 image、journal、receipt 與 claim。

## Trust 與資料邊界

Nodepool 是以下事項的唯一平台 authority：

```text
account execution authorization
Worker identity binding
lease 與 scheduling
proof request binding
receipt verification
verified usage
billing 與 settlement
audit lifecycle
```

Master、Worker 與 Worker sidecar 都是 untrusted caller 或 execution host，其 claim
必須由 Nodepool 驗證。PostgreSQL 與 Redis 保存平台狀態，只能由 Nodepool
後端使用，不得暴露給 Worker、Worker sidecar、browser 或下載的 package。

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
  - optional local managed-proof sidecar executable
  - no Nodepool private key or database credential

Worker package
  - no proof signing key
  - no Nodepool database credential
  - no HEADSCALE_API_KEY
  - no reusable proof bearer token
```

Worker execution tokens authenticate the assigned task attempt. They do not turn the
Worker or its sidecar into a settlement authority; only Nodepool can verify and settle.

## 失敗與安全規則

- enrollment credential 缺少或過期時，enrollment fail closed。
- Headscale 或 Nodepool readiness 失敗時，不得啟用 registration 或 task operation。
- local managed-proof sidecar 缺少、不可用、超時、不相容或回傳 malformed data 時，
  managed execution fail closed。
- stale lease、request digest 改變、Worker/owner 錯誤、attempt 錯誤、guest image
  錯誤、semantics 錯誤、response malformed、receipt 無效或 cryptographic
  verification 失敗時，不得產生可 billing 的完成結果。
- queue、proof、identity 與 cryptographic failure 不得被繞過；不使用 observe/off
  作為成功證據。
- source、input、receipt payload、password、raw JWT、Headscale key 與 private key
  不得寫入一般 log 或 durable state。
- Linux prover 不得複製進 native Windows package；沒有 proof-capable sidecar 時，
  managed task 必須 fail closed。

## 現有 Contract

- `proto/hivemind.proto` 定義 Worker/Master/Nodepool 共用的 gRPC surface。
- `hivemind-managed-prover-protocol` 定義獨立且有界的 Worker sidecar JSON protocol。
- 任務使用 `managed-function-v0` runtime：封閉的 source function 加上有界
  JSON input payload。
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
- Managed-prover Worker sidecar：本機 `MANAGED_PROVER_EXECUTABLE`；不存在獨立的 proving gRPC service。

以上是 development 或 service default，不是 live credential。
實際部署可以透過 environment variable 或 JSON config file 覆寫。

## 目前狀態

- Rust workspace 是 authoritative implementation。
- `docs_backup_20260611_202024/` 中較舊的 Python-era architecture notes
  只作為歷史參考。
- 本文件描述的公開 enrollment 與 dynamic-admission model 是目標產品
  架構。
- 目前 managed-prover implementation 仍使用靜態 Worker-ID capability
  configuration，以及手動 Worker sidecar 與 execution env provisioning。這些是過渡期或
  私有部署要求，不是理想的公開 onboarding 體驗。
- 目前 attestation 文件仍標記 guest-image equality gate 為 blocked；在 gate 通過前，
  不得宣稱已具備 trusted prover 或 managed settlement 證據。
- 完整證據仍需要在 Orange Pi 之外的 proof-capable Worker 完成 Website enrollment、
  Headscale overlay connectivity、Worker registration、local sidecar proof
  generation、Nodepool 獨立 receipt verification、result/log retrieval、usage、
  billing、settlement 與 audit evidence。
- 自動 client update/download 仍然延後處理。

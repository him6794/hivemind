# 最終產品目標與跨平台實作交接文件

> 這份文件是下一個 `/goal` 對話的工作入口。先讀本文件與
> `docs/ARCHITECTURE.md`，再檢查目前 worktree 的實際狀態。不要把本文件
> 中的「目標」誤認為目前已完成的功能。

## 一、最終產品目標

Hivemind 的最終使用體驗必須是「下載、登入、放著就能用」。

### Worker 使用者

```text
下載對應平台的 Worker client
  -> 登入 Website API
  -> 允許必要的背景執行權限
  -> 放著執行
```

登入後，平台自動完成：

- 建立或恢復 Worker identity。
- 取得短期 enrollment credential。
- 取得必要的網路/連線設定。
- 向 Nodepool 註冊 Worker 與 authenticated owner。
- 完成 capability/readiness handshake。
- 自動取得可用的 runtime 與平台政策。
- 在背景接收相容任務、執行 DSL、回報結果。

Worker 使用者不應該手動填寫或理解：

```text
Nodepool local executable path
Headscale local executable path
Worker ID
owner
Worker execution certificate path
trust list
lease
billing
settlement
```

### Master 使用者

```text
登入 Master client 或 Web UI
  -> 上傳已寫好的封閉 DSL 程式與 input
  -> 帳戶有足夠餘額
  -> 等待結果
```

Master 使用者不需要：

- 選擇某一台 Worker。
- 設定 Headscale 或 Nodepool。
- 管理 lease、retry 或結果驗證。
- 計算 usage、billing 或 settlement。
- 知道任務實際在哪一台主機執行。

平台自動負責：

```text
登入與 enrollment
  -> Worker admission
  -> quote/preflight
  -> distinct Worker 選擇
  -> lease 與 consensus round 排程
  -> closed DSL replica 執行
  -> canonical result comparison
  -> strict-majority quorum certificate
  -> fixed-reservation usage accounting
  -> billing / settlement
  -> result / log retrieval
```

Consensus certificates are agreement evidence between authenticated Workers,
not independent correctness validation or trusted usage attestation. Nodepool
settles only after the configured quorum is reached.

## 二、公開網路模型

平台是「登入後開放加入」而不是人工 allowlist 網路：

- 任何通過 Website API 登入的使用者都可以部署 Worker。
- Worker 透過 enrollment 自動註冊 Nodepool。
- Nodepool 綁定 Worker identity 與 authenticated owner。
- Nodepool 動態檢查 capability、liveness、quota、reputation、資源與政策。
- 不應要求 operator 手動將每一個 Worker 加入額外的本機 trust list。
- 不應要求使用者編輯 `HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES`。
- 不符合任務要求的 Worker 不被派發該任務，但可以正常加入網路。
- quota、rate limit、reputation、stake/bond、失敗懲罰等是平台政策，
  不是逐台 Worker 的人工 cryptographic allowlist。

Worker 每次以 Nodepool-issued execution token 接收已分派的 task attempt。
Consensus-enabled managed tasks 由 Nodepool 派送至 distinct Workers；token 綁定
round、replica、attempt、request digest 與完整 execution identity。Nodepool 只在
canonical result 達到 quorum 後完成 settlement。

Consensus-enabled managed tasks use only the Nodepool-coordinated replica
path. A task is settled only after matching canonical results reach the configured
quorum; if the quorum cannot be formed, the task remains unsettled or fails.

## 三、跨平台目標

需要加入網路的節點至少包含：

```text
Windows x64 / ARM64
Linux x64 / ARM64
macOS x64 / ARM64
Android ARM64
```

### 必須平台無關的部分

以下核心邏輯必須以純 Rust/標準 protocol 實作，不依賴特定作業系統 API：

- closed DSL 語義與執行結果。
- source/input/output bounds。
- task protocol 與 canonical serialization。
- Worker identity、task lease、attempt identity。
- Nodepool auth、consensus request binding 與 quorum certificate 驗證。
- canonical result serialization 與 distinct-replica observation protocol。
- cancellation、timeout、retry、idempotency 與 stale-attempt fencing。
- digest、budget、semantics 與 settlement reservation 驗證。
- result、observation、quorum certificate 與 Worker identity protocol。
- usage、billing、settlement 的資料模型。

核心不得依賴：

```text
Windows HCS
Linux cgroup
Docker
WSL
shell / arbitrary host command
平台專用 sandbox 語義
某個 OS 專用的 execution backend
```

managed DSL 必須是封閉 runtime，不能透過 DSL 呼叫任意 host process、
檔案系統、socket 或作業系統命令。

### 可以存在但必須隔離的 OS adapter

完全不使用任何 OS API 不符合實際 client 產品。每個作業系統仍然需要
很薄的 adapter 處理：

- process/app lifecycle。
- 背景執行限制。
- 安全 credential storage。
- 通知與 UI。
- 安裝、啟動與權限提示。

這些 adapter 不能改變 DSL、任務、consensus 或 settlement 語義。

Android 特別需要遵守 Android 的背景執行與電源限制，可能需要使用者
第一次允許 foreground service 或相關背景權限；這是一次性的 OS UX，
不能完全消除，但不能讓 Android 使用者手動配置平台網路與 consensus 流程。

## 四、建議的跨平台連線架構

若要求 Android 與桌面平台都能「下載、登入、放著跑」，Headscale VPN
interface 不應該是所有 client 的硬性依賴。因為建立系統 VPN interface
會分別依賴：

- Android `VpnService`。
- Windows 網路介面/服務機制。
- macOS Network Extension/權限。
- Linux tun/service 機制。

建議平台 client 的主要資料路徑改為應用程式層 outbound secure channel：

```text
Client 登入 Website API
  -> 自動取得短期 client identity
  -> Client 主動建立 HTTPS/HTTP2/QUIC/WebSocket channel
  -> Nodepool 透過 channel 派送任務與接收結果
```

這樣可以：

- 不需要使用者開 port。
- 不需要使用者操作 Headscale。
- 不需要作業系統 VPN interface 才能完成基本 Worker 工作。
- 適用於 NAT、桌面與 Android 背景環境。
- 將傳輸建立在跨平台的 Worker execution/HTTP2/QUIC/WebSocket protocol 上。

Headscale 可以保留為選配：

- Desktop/private deployment 的 overlay fast path。
- 需要 peer-to-peer routing 時使用。
- Operator 內部網路使用。
- 外部 Headscale 拓撲的相容模式。

但 Headscale 不應成為一般使用者或 Android client 的必要手動設定項目。
若堅持所有 client 都必須建立 Headscale VPN interface，Android 就必然
需要 Android VPN API，不能同時宣稱完全不依賴特定 OS API。

## 五、目前實作狀態與差距

### 已有基礎

- `managed-function-v0` closed DSL runtime 已存在。
- Worker 不執行任意 host command 的產品邊界已定義。
- Master、Nodepool、Worker 的主要任務流程已存在。
- Nodepool 是 identity、排程、consensus evaluation、usage、billing、
  settlement 的權威。
- consensus round、replica observation 與 quorum certificate 的 Rust 結構已存在，
  managed task 只走 Nodepool 協調的多 Worker 路徑。
- Native Windows Worker 執行 closed DSL 不需要 Linux service 或額外 runtime。
- `docs/ARCHITECTURE.md` 已記錄 Orange Pi 邊界、公開 enrollment 與
  zero-configuration 使用目標。

### 仍未達到最終目標

1. **Enrollment 還不是完整零設定流程**
   - **Task #5 已完成：** Website API 現在可在已驗證登入後簽發
     10 分鐘、角色限定、單次使用的 enrollment credential；只保存
     SHA-256 hash，並由 Nodepool 建立或恢復 owner/device/role identity。
   - Worker public dynamic registration 會使用 server-assigned owner 與
     Worker ID；重播、過期、錯誤角色/裝置與並發 redemption 會 fail closed。
   - **Task #6 已完成（wire contract + Nodepool routing）：** 新增
     `hivemind-client-core` 純 Rust session 核心（bounded hello/welcome/
     resume token、monotonic delivery sequence、ACK/result/cancellation
     idempotency、heartbeat expiry、redelivery）與 `WorkerSessionService`
     bidirectional streaming RPC。Nodepool 只保存 hashed resume token；
     Worker 透過短期 attempt-bound execution token 接收任務，斷線後以
     resume token 重連並 redeliver 未 ACK 任務；使用者取消任務會透過
     session cancel frame 傳遞並由 Worker 回報 cancellation ACK。
     Managed/general-compute runtime 仍走權威 unary path。
   - **Task #7 已完成（zero-config client migration）：** Worker UI 登入後
     自動完成 VPN/enrollment/registration/session 啟動；註冊不再要求可達的
     callback local executable path——空白 local executable path 會註冊為 session-only Worker，
     Nodepool 保留既有 callback address，任務透過 outbound session 派送與
     回報。兩個 UI 的 bearer JWT 改存 sessionStorage（關閉分頁即清除，
     不寫入持久 localStorage），並以 contract tests 鎖定此邊界。
   - **仍未完成：** 各平台 secure storage 與 adapter（Task #8），以及移除
     Headscale/VPN compatibility path 對現有部署的依賴；因此這一項不代表
     最終「完全零設定」產品已驗收。

2. **Public dynamic admission is implemented; private static mode remains optional**
   - Public Nodepool registration now accepts bounded, canonical Worker
     capability/readiness reports without a Worker-ID capability-map entry.
   - Dynamic observations persist admission mode, canonical capabilities,
     digest, readiness, reason and observation time; stale or tampered reports
     are excluded from scheduling and consensus request binding.
   - Public Worker mode validates Nodepool's per-attempt execution authorization
     plus its own bounded runtime/queue/image policy, without a permanent
     Worker-ID map.
   - `HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES` remains available only
     for explicitly selected private static deployments.

3. **Client 連線仍偏向 Headscale/平台特定實作**
   - **Task #6/#7 已完成部分：** outbound session wire contract 與 Nodepool
     routing 已存在（Worker execution/HTTP2/tonic bidirectional streaming）；Worker 端
     session loop 支援重連/backoff/resume；註冊 loop、UI login、enrollment
     與 session 啟動已整合，且註冊不再強制要求 callback local executable path。
   - Windows 使用 libtailscale/native DLL 的建置與部署路徑。
   - Android client 與背景服務尚未完成；Headscale 仍是現有部署的
     transport 依賴，尚未降級為純 optional overlay。
   - 需要將核心 protocol 與 OS/network adapter 分離。

4. **Linux/macOS/Android client 尚未達到一致產品體驗**
   - **Task #8 部分完成：** Windows x64/ARM64 package 契約已更新——package
     不要求 local executable path、static Worker ID 或 reusable nodepool token,並記錄
     session-only 預設行為；全工作區 `aarch64-pc-windows-msvc` cross-check
     通過。Linux/macOS 全工作區 cross-check 因本機缺少 Linux gcc 工具鏈而
     blocked(`hivemind-client-core` 純 Rust 核心已通過 Linux target check)。
   - 需要一致的下載、登入、enrollment、背景執行、更新與錯誤回報流程。
   - Android 需要獨立的 app/foreground-service adapter;Android FFI 與
     Linux/macOS packaging 定義尚未建立。

6. **Local managed consensus 完整鏈路尚未完成 live 驗證**
   - **已完成：** consensus policy、replica dispatch、canonical result
     comparison、strict-majority quorum certificate、Nodepool settlement
     guard 與資料庫持久化都有 focused tests。沒有足夠 distinct Workers 或
     無法形成 quorum 時，任務保持未結算或失敗，不會改走單一 Worker。
   - **仍 blocked：** 真正三個 distinct eligible Workers 的外部多節點執行、
     clean-host Worker enrollment、瀏覽器流程與完整結果/結算證據尚未在同一
     release run 中完成。Docker 或單進程測試只能驗證契約，不能冒充 live
     多節點證據；完成後應將命令、狀態與 settlement evidence 記錄在 validation
     state 文件。

7. **Worker/Nodepool 的跨平台自動化仍需完成 release 驗收**
   - Windows Worker 的登入、enrollment、registration、session 與 closed DSL
     執行路徑已提供 zero-config package；一般使用者不需要填 endpoint、port、
     Worker ID 或其他 runtime 設定。
   - 仍需在 clean Windows host 驗證雙擊啟動、登入後自動 enrollment、持續連線、
     任務接收與停止/重試行為；缺少必要 provider 或 quorum 時必須明確失敗。
   - Linux-only OCI services 只在 Nodepool/control plane 或本地 Docker 驗證，
     不得成為 Windows Worker 的隱性依賴。

8. **目前 dist package 不能當成正式 release**
   - 舊 package 可能有固定 Worker ID 或缺少設定。
   - 現有生成物可能標記 `git_dirty=true`。
   - 不應用本機 dist archive 推論 live Orange Pi readiness。

9. **完整外部 E2E 證據仍缺少**
   - Website login/enrollment。
   - Headscale 或 outbound secure channel。
   - Worker registration。
   - quote、排程與 DSL task。
   - distinct Worker replica execution 與 quorum certificate。
   - result/log retrieval。
   - Nodepool-owned usage、billing、settlement、audit evidence。

## 六、下一階段實作順序

下一個 `/goal` 應按照以下順序執行，不要只繼續堆疊手動設定：

### Phase 1：固定平台無關核心邊界

- 抽出或確認純 Rust Worker core。
- 將 DSL、task protocol、auth、consensus client、state、retry 與 idempotency
  保持在 core。
- 建立明確的 OS adapter interface。
- 確認 core 不引用 Windows HCS、Linux cgroup、Docker、WSL、shell 或
  OS-specific sandbox。

### Phase 2：完成登入後自動 enrollment

- Website API 提供短期、角色限定 enrollment credential。
- Client 登入後自動取得 Worker identity。
- 自動向 Nodepool 註冊 owner、capability 與 readiness。
- 自動取得必要的 runtime、consensus 與 transport configuration。
- 不再要求一般使用者填 local executable path、Worker ID、Worker execution path 或 trust list。
- 私密資料只放 secure storage，不能進 log、package 或一般 state。

### Phase 3：移除公開網路的 static trust list 依賴

- Nodepool 成為 Worker admission、task execution authorization、consensus
  evaluation 與 settlement 的唯一來源。
- Worker 只執行本機 bounded runtime contract，不接受網路傳入的結算決策。
- Worker capability/readiness policy 仍由 Nodepool 於派送前驗證。
- `HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES` 降級為 private
  deployment compatibility mode，不能阻擋公開 Worker enrollment。
- 保留所有 server-side capability、lease、identity 與 consensus request binding
  檢查。

### Phase 4：建立跨平台 outbound transport

- 以標準 Worker execution/HTTP2/QUIC/WebSocket 建立 client 主動連線。
- Nodepool 可以透過 outbound channel 派送任務與接收結果。
- Headscale 改為 optional overlay/private fast path，不作為所有 client
  的必要 VPN interface。
- 保留 Worker execution token 或等效的短期 client identity，但由 enrollment 自動取得。
- 在 Windows、Linux、macOS、Android 上使用相同的應用程式層 protocol。

### Phase 5：完成各平台 client

- Windows x64/ARM64：native client、登入、背景執行、secure storage。
- Linux x64/ARM64：daemon/service 與相同 Worker core。
- macOS x64/ARM64：background agent 與相同 Worker core。
- Android ARM64：app + foreground service adapter 與相同 Worker core。
- 平台差異只存在於 adapter、packaging、lifecycle、storage 與 UI。

### Phase 6：完成 consensus、settlement 與正式 E2E

- Nodepool 建立 durable consensus round，將相同的 deterministic task
  派送給足夠數量的 distinct Workers。
- Worker 只送出 bounded canonical result 與 attempt-bound identity。
- Nodepool 驗證 task、attempt、runtime、image、result digest 與 policy binding，
  再以 strict-majority quorum 建立 certificate。
- Worker 不取得 Nodepool database/private key，也不決定 settlement。
- 完成 duplicate、retry、cancel、timeout、stale lease、wrong result、
  Worker outage 與 settlement idempotency 測試。
- 用 fresh install client 驗證「只登入即可工作」的完整流程。

## 七、最終驗收標準

### Worker 驗收

在全新環境中，使用者只需：

```text
下載 client
登入
允許必要的背景執行權限
```

之後 Worker 必須自動：

- 完成 identity/enrollment。
- 連到平台。
- 顯示為 Nodepool 可用 Worker。
- 接收相容任務。
- 執行封閉 DSL。
- 在被派送時執行 consensus replica。
- 回報 canonical result 與執行狀態。

不得要求手動修改 config、local executable path、Worker ID、Worker execution path 或 trust list。

### Master 驗收

在全新環境中，使用者只需：

```text
登入 Master
上傳 DSL 與 input
確保帳戶餘額足夠
```

平台必須自動完成選 Worker、排程、replica 執行、quorum、計費與結算，並將
結果與必要的 logs 回傳給使用者。

### 平台驗收

- 任何通過 Website API 登入的使用者都能申請加入。
- 沒有人工逐台 Worker allowlist 才能加入的要求。
- Worker 與 client 都不能決定 settlement。
- Nodepool 是唯一 consensus、usage、billing、settlement authority。
- Windows、Linux、macOS、Android 使用相同的 core protocol 與 DSL 語義。
- 任何 OS-specific API 只出現在薄型 client adapter，不進入核心執行語義。
- Orange Pi 仍只執行 Nodepool、Website API、Headscale、PostgreSQL、Redis。
- Nodepool private key、Headscale API key 或 reusable credential 不得被打包
  進一般 client。

## 八、不可違反的工作區與安全限制

- 不要 reset、clean 或覆蓋其他未相關的 dirty worktree 變更。
- 不要刪除既有 legacy authorization data；本次變更不會對已部署資料庫執行 destructive drop。
- 沒有明確要求時不要 commit 或 push。
- 不要把 `HEADSCALE_API_KEY`、Nodepool private key、JWT、password 或 Worker execution
  private key 寫入 package、log 或文件。
- 不要把 Worker 或 Master 部署到 Orange Pi。
- 不要用 WSL、VM、Docker、SSH、socat 或 direct-host reachability 取代
  正式外部 Headscale/transport/consensus 證據。
- 不要把 `observe` 或 `disabled` 當成 production settlement 的 workaround；
  consensus 無法執行時必須保持未結算或失敗。
- 不要把本機 `dist/*`、ignored credential 或 archive 當成 live deployment
  evidence。

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
- 自動取得可用的 runtime、Provider 與平台政策。
- 在背景接收相容任務、執行 DSL、回報結果。

Worker 使用者不應該手動填寫或理解：

```text
Nodepool endpoint
Headscale endpoint
Provider endpoint
Worker ID
owner
TLS certificate path
proof token
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
- 設定 Headscale、Nodepool 或 Provider。
- 管理 proof token、lease、retry 或 receipt。
- 計算 usage、billing 或 settlement。
- 知道任務實際在哪一台主機執行。

平台自動負責：

```text
登入與 enrollment
  -> Worker admission
  -> quote/preflight
  -> Worker 選擇
  -> lease 與排程
  -> closed DSL 執行
  -> 必要的 remote proof
  -> receipt 驗證
  -> usage accounting
  -> billing / settlement
  -> result / log retrieval
```

## 二、公開網路模型

平台是「登入後開放加入」而不是人工 allowlist 網路：

- 任何通過 Website API 登入的使用者都可以部署 Worker。
- Worker 透過 enrollment 自動註冊 Nodepool。
- Nodepool 綁定 Worker identity 與 authenticated owner。
- Nodepool 動態檢查 capability、liveness、quota、reputation、資源與政策。
- 不應要求 operator 手動將每一個 Worker 加入 Provider trust list。
- 不應要求使用者編輯 `HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES`。
- 不符合任務要求的 Worker 不被派發該任務，但可以正常加入網路。
- quota、rate limit、reputation、stake/bond、失敗懲罰等是平台政策，
  不是逐台 Worker 的人工 cryptographic allowlist。

Provider 每次只接受 Nodepool 為特定 active attempt 簽發的短期 proof
authorization。Provider 驗證：

- issuer、audience、purpose、expiry。
- task、proof task、Worker、owner。
- execution、attempt、idempotency key。
- request digest、lease generation、deadline。
- runtime、backend、semantics。
- proof scheme 與 guest image。

Provider 不需要永久保存公開網路中每個 Worker 的人工信任資料。

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
- Nodepool auth 與 proof authorization 驗證。
- remote Provider protocol。
- cancellation、timeout、retry、idempotency。
- receipt、ExecutionClaim、digest、budget 與 semantics 驗證。
- usage、billing、settlement 的資料模型。

核心不得依賴：

```text
Windows HCS
Linux cgroup
Docker
WSL
shell / arbitrary host command
平台專用 sandbox 語義
某個 OS 專用的 RISC Zero backend
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

這些 adapter 不能改變 DSL、任務、proof 或 settlement 語義。

Android 特別需要遵守 Android 的背景執行與電源限制，可能需要使用者
第一次允許 foreground service 或相關背景權限；這是一次性的 OS UX，
不能完全消除，但不能讓 Android 使用者手動配置平台網路與 proof 流程。

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
- 將傳輸建立在跨平台的 TLS/HTTP2/QUIC/WebSocket protocol 上。

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
- Nodepool 是 identity、排程、proof verification、usage、billing、
  settlement 的權威。
- managed-prover protocol、proof authorization 與 Provider service 的
  主要 Rust 結構已存在。
- Native Windows Worker 不應攜帶 Linux prover 的方向已確立。
- `docs/ARCHITECTURE.md` 已記錄 Orange Pi 邊界、公開 enrollment 與
  zero-configuration 使用目標。

### 仍未達到最終目標

1. **Enrollment 還不是完整零設定流程**
   - **Task #5 已完成：** Website API 現在可在已驗證登入後簽發
     10 分鐘、角色限定、單次使用的 enrollment credential；只保存
     SHA-256 hash，並由 Nodepool 建立或恢復 owner/device/role identity。
   - Worker public dynamic registration 會使用 server-assigned owner 與
     Worker ID；重播、過期、錯誤角色/裝置與並發 redemption 會 fail closed。
   - **仍未完成：** outbound session、各平台 secure storage，以及移除
     Headscale/VPN compatibility path 對現有部署的依賴；因此這一項不代表
     最終「完全零設定」產品已驗收。

2. **Public dynamic admission is implemented; private static mode remains optional**
   - Public Nodepool registration now accepts bounded, canonical Worker
     capability/readiness reports without a Worker-ID capability-map entry.
   - Dynamic observations persist admission mode, canonical capabilities,
     digest, readiness, reason and observation time; stale or tampered reports
     are excluded from scheduling and proof authorization.
   - Provider public mode validates Nodepool's per-attempt authorization plus its
     own bounded runtime/queue/image policy, without a permanent Worker-ID map.
   - `HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES` remains available only
     for explicitly selected private static deployments.

3. **Client 連線仍偏向 Headscale/平台特定實作**
   - Windows 使用 libtailscale/native DLL 的建置與部署路徑。
   - Android client、背景服務與跨平台 outbound channel 尚未完成。
   - 需要將核心 protocol 與 OS/network adapter 分離。

4. **Linux/macOS/Android client 尚未達到一致產品體驗**
   - 需要一致的下載、登入、enrollment、背景執行、更新與錯誤回報流程。
   - Android 需要獨立的 app/foreground-service adapter。

5. **Remote proof 完整鏈路尚未完成 live 驗證**
   - Windows/Linux/macOS/Android Worker → Provider → Nodepool receipt
     verification → usage/billing/settlement 的完整證據尚未完成。

6. **Provider 部署仍有手動 operator 工作**
   - Provider env、TLS 憑證與 staged Linux x86_64 prover 仍需 operator 建立。
   - 這些設定不應暴露給一般 Worker 使用者，但必須有可重現的 operator
     deployment workflow。

7. **目前 dist package 不能當成正式 release**
   - 舊 package 可能有固定 Worker ID 或缺少設定。
   - 現有生成物可能標記 `git_dirty=true`。
   - 不應用本機 dist archive 推論 live Orange Pi readiness。

8. **完整外部 E2E 證據仍缺少**
   - Website login/enrollment。
   - Headscale 或 outbound secure channel。
   - Worker registration。
   - quote、排程與 DSL task。
   - remote proof。
   - Nodepool 獨立 receipt verification。
   - result/log retrieval。
   - verified usage、billing、settlement、audit evidence。

## 六、下一階段實作順序

下一個 `/goal` 應按照以下順序執行，不要只繼續堆疊手動設定：

### Phase 1：固定平台無關核心邊界

- 抽出或確認純 Rust Worker core。
- 將 DSL、task protocol、auth、proof client、state、retry 與 idempotency
  保持在 core。
- 建立明確的 OS adapter interface。
- 確認 core 不引用 Windows HCS、Linux cgroup、Docker、WSL、shell 或
  OS-specific sandbox。

### Phase 2：完成登入後自動 enrollment

- Website API 提供短期、角色限定 enrollment credential。
- Client 登入後自動取得 Worker identity。
- 自動向 Nodepool 註冊 owner、capability 與 readiness。
- 自動取得必要的 Provider/transport configuration。
- 不再要求一般使用者填 endpoint、Worker ID、TLS path 或 trust list。
- 私密資料只放 secure storage，不能進 log、package 或一般 state。

### Phase 3：移除公開網路的 static trust list 依賴

- Nodepool 成為 Worker admission 與 task authorization 的唯一來源。
- Provider 接受任何有效 Nodepool per-attempt proof authorization。
- Provider 只保留 runtime/backend/semantics/queue/quota 等自身能力政策。
- `HIVEMIND_MANAGED_DSL_TRUSTED_WORKER_CAPABILITIES` 降級為 private
  deployment compatibility mode，不能阻擋公開 Worker enrollment。
- 保留所有 server-side capability、lease、identity 與 proof binding 檢查。

### Phase 4：建立跨平台 outbound transport

- 以標準 TLS/HTTP2/QUIC/WebSocket 建立 client 主動連線。
- Nodepool 可以透過 outbound channel 派送任務與接收結果。
- Headscale 改為 optional overlay/private fast path，不作為所有 client
  的必要 VPN interface。
- 保留 mTLS 或等效的短期 client identity，但由 enrollment 自動取得。
- 在 Windows、Linux、macOS、Android 上使用相同的應用程式層 protocol。

### Phase 5：完成各平台 client

- Windows x64/ARM64：native client、登入、背景執行、secure storage。
- Linux x64/ARM64：daemon/service 與相同 Worker core。
- macOS x64/ARM64：background agent 與相同 Worker core。
- Android ARM64：app + foreground service adapter 與相同 Worker core。
- 平台差異只存在於 adapter、packaging、lifecycle、storage 與 UI。

### Phase 6：完成 proof、settlement 與正式 E2E

- Provider 使用 pinned Linux x86_64 RISC Zero sidecar。
- Worker 只送出 bounded canonical request。
- Nodepool 簽發 per-attempt authorization。
- Provider 不取得 Nodepool database/private key，也不決定 settlement。
- Nodepool 獨立驗證 receipt、ExecutionClaim、digest、budget、semantics。
- 完成 duplicate、retry、cancel、timeout、stale lease、wrong image、
  forged receipt、provider outage 與 settlement idempotency 測試。
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
- 必要時取得 remote proof。
- 回報結果。

不得要求手動修改 config、endpoint、Worker ID、TLS path 或 trust list。

### Master 驗收

在全新環境中，使用者只需：

```text
登入 Master
上傳 DSL 與 input
確保帳戶餘額足夠
```

平台必須自動完成選 Worker、排程、執行、proof、驗證、計費與結算，並將
結果與必要的 logs 回傳給使用者。

### 平台驗收

- 任何通過 Website API 登入的使用者都能申請加入。
- 沒有人工逐台 Worker allowlist 才能加入的要求。
- Worker 與 Provider 都不能決定 settlement。
- Nodepool 是唯一 proof、usage、billing、settlement authority。
- Windows、Linux、macOS、Android 使用相同的 core protocol 與 DSL 語義。
- 任何 OS-specific API 只出現在薄型 client adapter，不進入核心執行語義。
- Orange Pi 仍只執行 Nodepool、Website API、Headscale、PostgreSQL、Redis。
- 沒有 Linux prover、Nodepool private key、Headscale API key 或 reusable
  proof token 被打包進一般 client。

## 八、不可違反的工作區與安全限制

- 不要 reset、clean 或覆蓋其他未相關的 dirty worktree 變更。
- 不要刪除持久化 remote data。
- 沒有明確要求時不要 commit 或 push。
- 不要把 `HEADSCALE_API_KEY`、Nodepool private key、JWT、password、TLS
  private key 或 proof payload 寫入 package、log 或文件。
- 不要把 Worker 或 Master 部署到 Orange Pi。
- 不要用 WSL、VM、Docker、SSH、socat 或 direct-host reachability 取代
  正式外部 Headscale/transport/proof 證據。
- 不要用 `MANAGED_PROOF_ROLLOUT_MODE=off` 或 `observe` 當 production
  provider 缺失的 workaround。
- 不要把本機 `dist/*`、ignored credential、staged prover 或 archive 當成
  live deployment evidence。

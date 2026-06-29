# Hivemind 實用性與效能評估：對比 Golem Network 與 Akash Network

撰寫日期：2026-06-26
評估對象：本 repository 目前的 Rust 版 Hivemind、Golem Network、Akash Network

## 結論摘要

Hivemind 目前最適合定位為「可控 worker pool 的批次任務 runtime」，而不是立即對標成完整公網雲市場。和 Golem、Akash 相比，Hivemind 的優勢在於架構簡單、控制面可掌握、任務提交與 worker 執行鏈路短，適合先做私有/半私有算力池、校園/社群算力共享、受信任 provider 的批次工作。短期不建議直接把它包裝成 Akash 式泛用雲或 Golem 式全球開放任務市場，因為 marketplace、支付、資源驗證、供需撮合、SLA、容器隔離與跨網路 artifact delivery 仍未成熟。

建議路線：

1. **3 個月內先做「受控 provider network」**：鎖定可信 provider、批次 ZIP 任務、CPU 任務、可追蹤結果與失敗重試。
2. **6 個月內補齊效能與信任基礎**：端到端 benchmark、worker 能力校準、artifact 分發、任務隔離、排程觀測與成本模型。
3. **之後再評估公網化**：若要對標 Golem/Akash，需要先完成 provider onboarding、資源證明、計費結算、滥用防護與公開 marketplace。

## 評估範圍與證據

本評估使用三類證據：

- Hivemind 當前程式碼與 docs：
  - `README.md`
  - `docs/ARCHITECTURE.md`
  - `hivemind-rs/crates/task-scheduler/src/scheduler.rs`
  - `hivemind-rs/crates/task-scheduler/src/dispatcher.rs`
  - `hivemind-rs/crates/worker-executor/src/executor.rs`
- 官方/公開資料：
  - Golem Network Stats：<https://stats.golem.network/>
  - Golem Stats API：<https://api2.stats.golem.network/>
  - Golem Docs：<https://docs.golem.network/>
  - Akash Docs：<https://akash.network/docs/>
  - Akash Console API provider endpoint：<https://console-api.akash.network/v1/providers>
- 2026-06-26 抓取的即時網路快照：
  - Akash：`https://console-api.akash.network/v1/providers`
  - Golem：`https://api2.stats.golem.network/v2/network/historical/stats`、`/v1/network/utilization`、`/v1/network/earnings/overviewnew`

限制：

- Hivemind repository 目前沒有 Criterion/負載測試或端到端 benchmark 檔案；效能評估以架構瓶頸、控制面複雜度、已知程式碼路徑與公開網路快照推導。
- Golem/Akash 的公開 provider 數與可用資源會快速變動，本報告中的網路快照只代表 2026-06-26 抓取時點。
- 沒有在三個網路上執行同一個實際 workload；因此本文不聲稱 Hivemind 在真實任務執行時間上優於或劣於 Golem/Akash，只比較目前設計可預期的效能特性與產品成熟度。

## 即時網路快照

### Akash Network

2026-06-26 透過 Akash Console API 抓取：

| 指標 | 數值 |
|---|---:|
| provider 總數 | 1,768 |
| online provider | 65 |
| online GPU provider | 37 |
| GPU total | 264 |
| GPU available | 130 |
| GPU active | 127 |

前 10 個線上 GPU model 分布：

| GPU model | provider count |
|---|---:|
| rtx3090 | 6 |
| h100 | 4 |
| a100 | 4 |
| rtx4090 | 3 |
| rtx4070 | 2 |
| rtx4000ada | 2 |
| rtx5070 | 2 |
| h200 | 2 |
| p40 | 2 |
| t4 | 2 |

Akash 的實用優勢是現有 Kubernetes/container deployment model、provider 屬性、GPU 型號、persistent storage、public IP、自訂 domain 等能力都已進入公開 marketplace 流程。對 Hivemind 而言，Akash 是「泛用雲部署與 GPU 租用」方向的主要參照。

### Golem Network

2026-06-26 透過 Golem Stats API 抓取 `vm` runtime：

| 指標 | 數值 |
|---|---:|
| online provider | 455 |
| CPU cores | 4,706 |
| memory | 10.63 TB |
| disk | 88.7 TB |
| GPUs in `vm` runtime | 0 |
| providers computing | 361 |
| network earnings 24h | 137.64 GLM |
| network earnings 7d | 963.02 GLM |
| network total earnings | 285,133.58 GLM |

Golem 的實用優勢是 requestor/provider 任務市場、Yagna runtime、GLM 支付與長期運作的公開 task marketplace。對 Hivemind 而言，Golem 是「公網批次任務市場」方向的主要參照。

## Hivemind 當前能力摘要

目前 Hivemind 是 Rust workspace，單一 `hivemind-bin` 可跑 master API、nodepool、worker，主要元件如下：

- Master HTTP API：登入、任務提交、查詢與 admin API。
- Nodepool gRPC：worker registration、heartbeat、task scheduling。
- Task Scheduler：依 worker 狀態、資源、價格上限與 cache affinity 分派任務。
- Worker Executor：在 sandbox 目錄解包與執行任務，回報結果。
- Redis/PostgreSQL：共享狀態、任務、worker、trust、audit。
- Torrent/artifact 支援：目前已有 ZIP/torrent metainfo 與本地 artifact 路徑處理。

實用性亮點：

- 對「可信 worker 的批次任務」已經有完整骨架。
- 排程條件包含 CPU score、GPU score、RAM、VRAM、storage、provider enable、provider resource limits、最低價格與 task `max_cpt`。
- 已有 heartbeat、redispatch、timeout、worker trust、stale worker 防護與 admin observability 入口。
- Rust 單 binary 部署對 provider onboarding 比多服務系統簡單。

主要缺口：

- worker-executor 目前對遠端 torrent/artifact 的 fetch 還未完成；若 worker 本地沒有 task artifact，會回報「remote torrent/artifact fetch is not implemented」。
- 沒有公開 marketplace、撮合與計費結算的完整閉環。
- 沒有硬體能力校準/防作弊的標準化 benchmark。
- 沒有端到端效能 benchmark、壓測、SLO 或容量模型。
- sandbox 能力仍需和公網 hostile workload 的威脅模型對齊。

## 實用性對比

| 面向 | Hivemind | Golem Network | Akash Network | 判斷 |
|---|---|---|---|---|
| 核心定位 | 批次任務 runtime / worker pool | 公網任務 marketplace | 去中心化 Kubernetes/cloud marketplace | Hivemind 應先走受控批次 runtime，不宜直接承諾泛用雲 |
| 工作負載 | ZIP/task package、命令執行、批次任務 | requestor/provider task execution | container deployment、service/VM-like workloads、GPU | Hivemind 更接近 Golem 的 batch，不接近 Akash 的服務部署 |
| Provider onboarding | 可打包 worker binary，但仍偏手動 | Yagna provider 流程成熟 | provider/operator 流程成熟且有公開屬性 | Hivemind 需補 onboarding、health check、資源校準 |
| 資源描述 | CPU/GPU score、RAM/VRAM/storage、price cap | provider offer / runtime capabilities | SDL、provider attributes、GPU model、storage/IP/domain | Hivemind 資源模型已有雛形，但需要標準化能力證明 |
| 使用者體驗 | Master UI / CLI / HTTP API | SDK/CLI requestor 模式 | SDL manifest、console、CLI | Hivemind 適合先做內部平台 UX |
| 支付/結算 | CPT 欄位與 budget guard 雛形 | GLM payment 已運作 | AKT/lease marketplace 已運作 | Hivemind 目前不可視為已完成經濟層 |
| 信任/治理 | worker trust/admin control 雛形 | 公網 reputation/market behavior | audited attributes、provider status | Hivemind 有內控優勢，但公網信任不足 |
| 運維可控性 | 高，因為中心化 master/nodepool | 中，公網 provider 變異大 | 中，依 provider lease | Hivemind 在小型可信網路中更好控 |
| 公網可擴展性 | 未證明 | 已有公網供需 | 已有公網供需與 GPU | Hivemind 還不應宣稱可直接公網規模化 |

### 實用性評分

評分：1 = 尚未可用，3 = 可在限制條件下使用，5 = 成熟可公開使用。

| 面向 | Hivemind | Golem | Akash | Hivemind 評語 |
|---|---:|---:|---:|---|
| 受控批次任務 | 3 | 4 | 3 | 已有核心 runtime，但 artifact delivery 與 benchmark 需補 |
| 公開 marketplace | 1 | 5 | 5 | Hivemind 尚無公開供需與成熟結算 |
| Provider onboarding | 2 | 4 | 4 | Windows package/worker binary 有雛形，但缺資源校準與自助流程 |
| GPU 實用性 | 2 | 2 | 5 | Hivemind 有欄位與監控基礎；Akash 已有公開 GPU provider 資源 |
| 成本控制 | 2 | 4 | 4 | Hivemind 有 `max_cpt`/`min_cpt_per_hour`，但未形成結算閉環 |
| 可觀測性 | 3 | 4 | 4 | Hivemind 有 admin API 與 cache metrics；仍缺端到端 latency/throughput dashboard |
| 公網安全 | 1 | 4 | 4 | sandbox 與 hostile workload 模型仍需補強 |

## 效能評估

### 控制面效能

Hivemind 的控制面路徑短：Master API -> Nodepool gRPC -> Worker gRPC。相對 Akash 的 blockchain lease / provider bid / Kubernetes deployment，Hivemind 在受控網路中理論上可有更低的任務分派延遲；相對 Golem 的公網 market negotiation，Hivemind 也可用中心化排程降低決策成本。

但目前 scheduler 實作也有明確瓶頸：

- `dispatch_pending` 對 pending tasks 逐一 dispatch。
- 每個 task 會對候選 workers 排序；排序為每 task 約 `O(W log W)`。
- cache affinity 會對每個 task 查詢歷史任務資料。
- 任務與 worker 狀態依賴 PostgreSQL，控制面容量會被 DB pool、查詢索引、pending task 數與 worker 數限制。

因此目前設計適合數十至數百 worker、低至中等 task queue 的受控網路。若要走到千級 worker / 萬級 pending tasks，需要把排程從「每任務查詢與排序」升級成：

- worker eligibility cache；
- resource bucket / index；
- batch dispatch；
- cache affinity 預聚合；
- DB query plan 與索引驗證；
- dispatcher concurrency 與 backpressure。

### 執行面效能

Hivemind worker executor 的優勢是直接在本機 worker 上啟動任務程序，對單次批次任務的 runtime overhead 應低於完整 Kubernetes deployment。這使它適合：

- 短到中等時間的批次任務；
- CPU-heavy job；
- 可解包成工作目錄的 artifact；
- 可信 provider 上的反覆任務。

目前限制：

- artifact delivery 未完整公網化；遠端 artifact fetch 未完成會直接限制跨 worker 任務可用性。
- resource isolation 和 hostile workload 防護仍需驗證。
- GPU 支援有資源欄位與 resource monitor，但尚未看到完整 GPU workload lifecycle、driver/runtime compatibility、GPU isolation、CUDA image 管理與 benchmark 方案。

### 網路與資料分發效能

Hivemind 目前以 ZIP/torrent metainfo 作 artifact 基礎，但 worker executor 還依賴本地 artifact path。這代表現階段的資料分發效能不是「去中心化 swarm 已可用」狀態，而是「artifact 模型已開始，但遠端 fetch 還需實作」。

和 Akash/Golem 對比：

- Akash 的強項是 container image + Kubernetes deployment；artifact/image delivery 走成熟 container registry 生態。
- Golem 的強項是 task package/runtime marketplace；適合大量獨立小任務。
- Hivemind 若補齊 torrent/swarm 或 cache-aware artifact fetch，可在重複資料集任務上建立優勢，因為 scheduler 已有 cache affinity 的設計。

### 成本/價格效能

Hivemind 目前有 `max_cpt`、`min_cpt_per_hour` 與 budget guard 的雛形，但還缺：

- CPT 的法幣/代幣換算；
- worker 報價市場；
- escrow / settlement；
- usage metering 防作弊；
- provider 收益與 requestor 成本報表。

因此短期只能把 CPT 當內部點數或配額，不應對外宣稱為成熟 marketplace 價格系統。

### 效能成熟度評分

評分：1 = 未驗證，3 = 架構可行但需 benchmark，5 = 已有公開/長期運行證據。

| 面向 | Hivemind | Golem | Akash | Hivemind 評語 |
|---|---:|---:|---:|---|
| 任務分派延遲潛力 | 4 | 3 | 2 | 中心化 gRPC 鏈路短；但缺實測 |
| 大規模排程證據 | 2 | 4 | 4 | Hivemind 未有壓測；Golem/Akash 有公網運行證據 |
| 單任務執行 overhead | 4 | 3 | 2 | 直接本機 process 啟動理論上較輕；需實測 |
| 資料分發 | 2 | 3 | 4 | Hivemind artifact 模型未完整；Akash 受益 container registry |
| GPU workload | 2 | 2 | 5 | Hivemind 尚未形成 GPU lifecycle；Akash GPU marketplace 較成熟 |
| 故障恢復 | 3 | 4 | 4 | Hivemind 有 heartbeat/redispatch；缺 chaos benchmark |
| 成本/效能可比性 | 1 | 4 | 4 | Hivemind 尚無標準化計價與 usage metering |

## 產品定位建議

### 不建議的定位

不要短期宣稱：

- 「Akash 替代品」：Hivemind 還不是泛用 container cloud，也沒有 lease/provider marketplace。
- 「Golem 替代品」：Hivemind 還沒有公網 task market、payment 與成熟 provider 流程。
- 「可直接接 hostile 公網 worker」：sandbox、資源驗證、artifact 安全、濫用防護都還要補。

### 建議定位

建議定位為：

> Hivemind is a Rust-based distributed batch runtime for controlled compute pools, with a path toward public provider marketplaces after resource verification, artifact distribution, metering, and trust controls mature.

中文產品說法：

> Hivemind 是面向可信或半可信算力池的分散式批次任務 runtime。先解決受控 provider 的任務分發、執行、結果回報、成本上限與觀測，再逐步演進到公開 marketplace。

## 後續規劃

### Phase 1：可用 MVP（0-6 週）

目標：讓小型可信 worker pool 穩定完成批次 ZIP 任務。

- 完成 worker 遠端 artifact fetch：
  - 支援 master API 上傳後 worker 可下載 artifact；
  - 若使用 torrent，完成 worker 端 magnet/torrent fetch；
  - 驗證 BTIH / checksum。
- 建立端到端 smoke benchmark：
  - 1 worker / 10 tasks；
  - 5 workers / 100 tasks；
  - 測量 submit latency、dispatch latency、start latency、wall time、failure rate。
- 補 task lifecycle metrics：
  - queued -> assigned；
  - assigned -> running；
  - running -> completed/failed；
  - redispatch count；
  - artifact download bytes/time。
- 明確限制公網能力：
  - docs 中標註 production/public-network 尚未完成；
  - 預設 provider onboarding 走 allowlist。

### Phase 2：效能與可靠性（6-12 週）

目標：讓 Hivemind 在 50-200 worker / 1k-10k pending task 的控制面可預測。

- 排程效能：
  - 對 `tasks.status`、`tasks.worker_id`、`tasks.torrent_source`、`tasks.completed_at`、`worker_nodes.status` 補查詢分析與必要索引；
  - cache affinity 改為預聚合或批次查詢；
  - pending tasks 分批 dispatch；
  - worker eligibility cache。
- Worker 能力校準：
  - 建立 CPU benchmark score；
  - 建立 GPU benchmark score；
  - 記錄 GPU model、VRAM、driver/CUDA capability；
  - 防止 worker 自報資源過度膨脹。
- 可靠性：
  - worker crash / heartbeat lost / stale completion 的 chaos test；
  - task idempotency 與 side-effect policy；
  - artifact cleanup 與 quota。

### Phase 3：受控市場化（3-6 個月）

目標：在可信 provider 網路中引入準市場機制。

- Provider profile：
  - location；
  - hardware；
  - network speed；
  - uptime；
  - completed task rate；
  - failure/retry rate。
- Pricing：
  - CPT 定義；
  - requestor budget；
  - provider min price；
  - usage-based billing；
  - admin settlement report。
- Trust：
  - reputation score；
  - dispute/audit hooks；
  - provider ban/limit；
  - result verification for deterministic tasks。

### Phase 4：公網化評估（6 個月後）

只有在 Phase 1-3 的 benchmark 與穩定性達標後，再評估：

- 是否接入代幣/支付；
- 是否開放非 allowlist provider；
- 是否提供 Akash-like deployment；
- 是否提供 Golem-like SDK；
- 是否需要多 master / federation / decentralized scheduler。

## 建議的效能 benchmark 規格

為了讓未來能和 Golem/Akash 做實測對比，建議新增 `benchmarks/` 或 `docs/perf/`，定義以下固定 workload：

| Benchmark | 目的 | 指標 |
|---|---|---|
| `cpu-short` | 短任務排程 overhead | submit-to-start p50/p95、task/s |
| `cpu-long` | 長任務穩定性 | completion rate、redispatch rate、worker utilization |
| `artifact-small` | 小 artifact 分發 | download latency、cache hit rate |
| `artifact-large` | 大 artifact 分發 | throughput、failure rate、disk pressure |
| `gpu-smoke` | GPU capability 驗證 | GPU detect success、CUDA startup latency |
| `worker-churn` | worker 掉線/重連 | timeout latency、redispatch success |
| `scheduler-scale` | 控制面容量 | pending queue size、DB CPU、dispatch loop duration |

每次 release 前至少記錄：

- git commit；
- worker 數；
- task 數；
- task payload size；
- DB/Redis 配置；
- p50/p95/p99 latency；
- successful / failed / redispatched task count；
- CPU/memory/DB query metrics。

## 和 Golem / Akash 的追趕優先級

優先級排序：

1. **先追 Golem 的 batch practicality，不追 Akash 的泛用 cloud**
   Hivemind 現有模型更接近 batch task runtime；要變成 Akash 類 Kubernetes marketplace 會大幅增加 scope。

2. **先做 artifact delivery 與 benchmark，再做 token/payment**
   沒有穩定執行與可測效能，經濟層只會放大風險。

3. **先受控 provider，再公網 provider**
   目前 trust-control 是優勢，應先用 allowlist provider 建立可運營案例。

4. **GPU 先做檢測和 smoke，不急著承諾 GPU marketplace**
   Akash 已經有公開 GPU provider 屬性與多種 GPU model；Hivemind 需要先完成 GPU lifecycle 和隔離能力。

## 最終建議

Hivemind 應採取「Golem-like batch runtime 的受控版本」作為近期產品方向：

- 近期對外展示：可信 worker pool、ZIP 任務提交、排程、結果回報、重試、成本上限與 dashboard。
- 中期對標 Golem：requestor SDK、任務市場、provider reputation、批次 workload benchmark。
- 遠期再對標 Akash：container deployment、GPU marketplace、persistent service workloads。

如果工程資源有限，下一個最有效的 milestone 是：

> 在 5 台 worker 上穩定跑 100 個 artifact-based CPU 任務，輸出完整 benchmark 報告，並證明任務可以在 worker 掉線後自動 redispatch。

這個 milestone 能直接驗證 Hivemind 的核心價值，也能暴露排程、artifact、worker reliability、observability 的真實瓶頸。

## 附錄：資料抓取命令

Akash provider/GPU 快照：

```powershell
$providers = Invoke-RestMethod -Uri 'https://console-api.akash.network/v1/providers' -TimeoutSec 30
$online = @($providers | Where-Object { $_.isOnline })
$gpuProviders = @($online | Where-Object { $_.gpuModels -and $_.gpuModels.Count -gt 0 })
$gpuTotal = 0; $gpuAvailable = 0; $gpuActive = 0
foreach ($p in $online) {
  if ($p.stats -and $p.stats.gpu) {
    $gpuTotal += [int]$p.stats.gpu.total
    $gpuAvailable += [int]$p.stats.gpu.available
    $gpuActive += [int]$p.stats.gpu.active
  }
}
$models = $gpuProviders |
  ForEach-Object { $_.gpuModels } |
  Group-Object model |
  Sort-Object Count -Descending |
  Select-Object -First 10 Name,Count
```

Golem network 快照：

```powershell
$hist = Invoke-RestMethod -Uri 'https://api2.stats.golem.network/v2/network/historical/stats' -TimeoutSec 30
$util = Invoke-RestMethod -Uri 'https://api2.stats.golem.network/v1/network/utilization' -TimeoutSec 30
$earn = Invoke-RestMethod -Uri 'https://api2.stats.golem.network/v1/network/earnings/overviewnew' -TimeoutSec 30
$latest = $hist.vm.'1d' | Select-Object -Last 1
$computing = [double]($util.data.result[0].values | Select-Object -Last 1)[1]
```

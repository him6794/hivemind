# Managed Runtime 演進計畫：圖靈完備與科學運算

## 0. 先講結論

目前的 `managed-function-runtime` 是刻意設計成 deterministic、bounded、可計量的 JSON DSL。它適合做可驗證的函式計費與 proof settlement，但不能誠實宣稱是「部署時圖靈完備」或「完整科學運算環境」：目前的值域只有有限的整數、布林、字串、list、dict、null，且每次執行有固定的 operation、loop、call-depth、value、output 與 materialization 上限。

本計畫不把這兩個互相衝突的目標硬塞進同一個 v0 執行器，而是採雙 runtime 契約：

| Runtime | 目的 | 宣稱 | 結算方式 |
|---|---|---|---|
| `managed-function-v0` | 小型 deterministic 函式、可 proof／meter 的結算 | 有限 DSL；不宣稱圖靈完備 | 保留現有 proof settlement |
| `general-compute-v1` | 一般程式與科學工作負載 | 語言語義具備圖靈完備性；單次執行仍受資源配額限制 | 先採可驗證執行／複製執行／attestation；不強制套用現有 RISC Zero 路徑 |

「圖靈完備」在這裡是語言的抽象語義宣稱，不是允許生產任務無限消耗 CPU 或記憶體。任何實際平台都必須有 CPU、記憶體、wall-time、取消、儲存與輸出上限；非終止程式應被可預期地取消，而不是拖垮 Worker。

## 1. 目標與不可接受的結果

### 目標

- 保持 v0 的 deterministic、bounded、可重播與 proof 相容性。
- 增加隔離的 v1 general-compute backend，能執行任意迴圈、遞迴、可成長 heap 與任意精度整數的程式。
- 提供可實際使用的 CPU 科學運算：浮點、複數、ndarray、線性代數、FFT、統計、ODE、Monte Carlo 與 sparse matrix/tensor。
- 讓大型輸入／輸出走 binary artifact、chunked transfer、checksum 與 content-addressed storage，不把 ndarray 塞進 JSON 或 gRPC 單一訊息。
- 使資源、版本、輸入輸出雜湊、seed、backend capability 與結果狀態可被 Nodepool 驗證及計費。
- 每個里程碑都有可重跑的測試、benchmark、fixture 或安全證據；沒有測試證據就不能把功能標成完成。

### 不可接受

- 把 `ExecutionLimits::unlimited()` 當成生產預設，或以有限 counter 宣稱 v0 已圖靈完備。
- 讓 Worker 自稱的 CPU、記憶體、GPU、輸出或 billing 數字直接成為結算事實。
- 允許任意 `pip install`、任意網路、任意 host filesystem 或任意 native library 載入。
- 為了科學運算移除 sandbox、取消、output cap 或 hash pinning。
- 在未定義 NaN/Inf、dtype、stride、byte order、seed 與誤差容忍度前，宣稱「支援 NumPy/SciPy」。
- 讓 v1 的巨大執行軌跡強制通過目前約 570–580 秒的單次 RISC Zero proof path。

## 2. 目標架構

### 2.1 契約分層

新增 `general-compute-v1`，不改變 `managed-function-v0` 的語義。Rust control plane 負責驗證請求、建立 sandbox、套用配額、串流 artifact、取消／kill／reap、收集 telemetry 與產生結果 envelope；guest/backend 負責執行使用者程式。

第一個實用 backend 採用固定 digest 的 Linux OCI image：Python 3.12、固定版本的 NumPy/SciPy、BLAS/LAPACK、FFT backend 與必要的 native runtime。這能先提供科學使用者熟悉且完整的語言與函式庫；backend 介面保留給後續 WASI、Rust、Julia 或 GPU image，不把 Python 套件直接編進 Nodepool。

建議的底層邊界：

```text
task-scheduler / nodepool
        │ versioned RuntimeRequest + artifact manifests
        ▼
general-compute-runtime  (Rust supervisor library)
        │ bounded stdin/stdout protocol, no shell interpolation
        ▼
rootless sandbox / pinned guest image
        │ Python + NumPy/SciPy or another registered backend
        ▼
RuntimeResult + usage + output artifact manifests
```

建議的程式分層：

- `executor-rs/crates/managed-function-runtime`：維持 v0 DSL、canonical renderer、metering 與 proof guest 共用語義。
- `executor-rs/crates/general-compute-runtime`：新增 request/result schema、supervisor、quota、cancellation、artifact、capability 與 backend adapter；不得依賴 Hivemind database。
- `hivemind-rs/crates/worker-executor`：依 runtime version 分派 v0 或 v1；保留 detached supervisor 的 kill/reap cleanup guard。
- `proto/hivemind.proto`：只傳版本化 manifest、hash、狀態與受限 metadata；大型資料走 artifact service/CAS。
- `packaging/` 與 Docker/worker package：以 image digest、backend manifest、driver compatibility matrix 產生可重現包。

### 2.2 Runtime request/result 契約

`general-compute-v1` 的請求至少包含：

- `runtime_version`、guest image digest、backend id、entrypoint 與 source/package artifact digest。
- input manifest：大小、MIME/type、shape/dtype metadata、chunk hashes、整體 checksum。
- `ExecutionPolicy`：CPU quota、memory limit、wall-time、最大 process/thread、暫存空間、output bytes、network/filesystem/GPU capability、取消 deadline。
- `DeterminismPolicy`：seed、thread count、CPU feature set、reproducible／best-effort 模式。
- billing version 與 cost model version。

結果至少包含：

- `completed`、`failed`、`cancelled`、`timed_out`、`resource_exhausted`、`backend_unavailable` 等明確狀態。
- exit/error code、受限 stdout/stderr、output artifact manifest、output hash 與 checksum。
- 實測 CPU time、wall time、peak memory、I/O bytes、GPU time/VRAM（皆標註為 worker claim，未驗證前不得結算）。
- runtime/backend/image/cost-model 版本、input hash、seed、reproducibility flag 與 capability negotiation 結果。

## 3. 圖靈完備性工作包

### G0：定義語義與紅測試

先寫 reference interpreter／formal test fixtures，再寫最佳化 backend。語言層必須明確具備：

- 任意迴圈（`while` 或等價控制流）。
- 遞迴與至少可表達的函式／閉包呼叫。
- 可成長的 mutable heap、條件分支與狀態更新。
- 抽象語義中的任意精度整數；實際執行由 memory/time quota 終止。
- 明確的 exception、exit、cancel 與 resource-exhausted 語義。

驗收：兩計數器（Minsky machine）可編碼並通過加法、遞減、零測試與 halt fixture；有限 budget 時在固定 counter 位置得到相同 `resource_exhausted`；非終止 fixture 在 deadline 內收到 cancellation 且 process 已 kill/reap；reference interpreter 與 production backend 做 differential tests。

### G1：核心 backend 與資源邊界

- 實作 `general-compute-runtime` supervisor 與版本化 wire protocol。
- 以 monotonic clock、cgroup/job object、process group、memory limit 與 output pipe cap 套用配額。
- cancellation 必須是 cooperative request 加 hard kill；任何 future drop、Worker shutdown、timeout 都必須完成 child reap 後才釋放 concurrency slot。
- stdin/stdout/stderr 都有上限與 back-pressure；錯誤不得把 source、input 或 secret 放進 command line 或公開 log。
- parser、loader、ABI decoder 與 protocol decoder 加入 fuzz target。

驗收：100% 的 timeout/cancel/child-exit path 沒有 orphan process、hung task 或 semaphore leak；同一 input/image/seed 在 reproducible profile 重跑得到相同 output hash；hostile workload 不能突破 memory、wall-time、output、filesystem 或 network policy。

## 4. 科學運算工作包

### S1：數值型別與資料 ABI

定義 versioned binary tensor envelope，不用 JSON 表示大型數值資料：

- scalar：`float32`、`float64`、`complex64`、`complex128`、signed/unsigned integer、arbitrary precision integer；明確支援 IEEE-754 NaN、`+Inf`、`-Inf`、signed zero 與 byte order。
- ndarray metadata：`dtype`、shape、stride、offset、layout（C/F/非連續 view）、read-only/mutable、chunk shape 與 storage hash。
- 互通：CPU 先支援 NumPy `.npy/.npz` 或等價 versioned envelope；程序內優先採 Arrow C Data Interface，device buffer 採 DLPack；避免未驗證的 pickle。
- sparse：CSR、CSC、COO 的 index dtype、index base、shape、indptr/indices/data checksum 必須寫入 manifest。
- chunked artifact：每 chunk 有 hash，整體有 Merkle/root checksum，支援 resume、range fetch、CAS dedup 與大小上限。

驗收：空陣列、零維陣列、非連續 slice、負 stride、NaN/Inf、complex、超大整數、endian conversion、稀疏空列與 malformed metadata 都有 golden tests；native、guest、artifact round-trip 的 metadata 與 hash 一致。

### S2：核心運算與數值正確性

按可驗收順序交付：

1. broadcasting、slice/view、elementwise unary/binary、dtype promotion、reduce、累積誤差規則。
2. `dot`、matmul、batched matmul、LU、solve、QR、SVD；CPU BLAS/LAPACK backend 必須 pin 版本並記錄 thread/CPU feature。
3. FFT/IFFT、real/complex transform、normalization convention 與 round-trip error。
4. 統計與 RNG：mean/variance/quantile、分布抽樣、seeded random、stream splitting；每個 nondeterministic backend 都要標記而非假裝 deterministic。
5. ODE：至少固定步長 RK4 與一個 adaptive solver，輸出 solver status、step count、tolerance 與 failure reason。
6. Monte Carlo：seed、sample count、confidence interval、parallel reduction policy 與可重播 fixture。
7. sparse matrix/tensor 基本 algebra、solve、reduce 與格式轉換。

數值驗收門檻：

- f64 穩定運算與 NumPy/SciPy/reference 結果比較，預設相對／絕對誤差門檻為 `1e-12`；f32 預設 `1e-5`；complex component-wise 使用對應門檻。
- LU/QR/solve 額外檢查 residual、orthogonality 或 backward error；不能只比較最後幾個數字。
- FFT round-trip、ODE known solution、Monte Carlo confidence coverage 與 sparse reference fixtures 必須在 CI 通過。
- ill-conditioned、overflow、underflow、NaN/Inf propagation 與 singular matrix 必須有明確 status，不能靜默產生成功結果。

### S3：GPU capability

- capability negotiation 必須包含 GPU vendor、compute capability、CUDA/ROCm runtime、driver ABI、VRAM、可用 stream 與 image digest。
- CUDA、ROCm 使用不同且固定的 image/driver compatibility matrix；不允許只看「有 GPU」就派送。
- 沒有相容 GPU 時只能明確回 `backend_unavailable` 或按 policy 使用 CPU fallback，結果需記錄實際 backend。
- GPU buffer 的輸入輸出仍以 checksum、大小、dtype、shape 與 device metadata 驗證；超過 VRAM 要可分塊或明確失敗。

## 5. 安全、信任與計費

v1 延續目前 trust model：Nodepool 是唯一可信結算權威，Worker 的結果與 resource usage 都是 claim。

- image 以 digest pinning；source/package、input、output、依賴 lockfile 與 backend manifest 都以 hash 綁定。
- image non-root、read-only root filesystem、短生命週期 scratch、最小 capabilities；Linux 使用 namespace、seccomp、cgroup、`no_new_privs` 或等價隔離，Windows 使用受支援的 job object/ACL/程序隔離能力。
- network 預設 deny；filesystem 只允許明確的 read-only input 與 ephemeral output mount；禁止任意 pip/npm install、動態 native plugin 與 host socket。
- Worker 只回傳受限 telemetry；Nodepool 驗證 envelope、hash、配額、runtime/image/cost-model 版本後才建立 usage claim。
- v0 繼續走目前 proof settlement。v1 首版採 replicated execution、可信 attestation 或可插拔 proof backend；必須把 verifiability level 寫進結果，不能把未驗證 claim 當成 proof。
- 每個版本同時鎖定 runtime version、cost model、proof/attestation protocol、guest image、trust pin 與 golden fixtures；任一項變更都產生新版本。

## 6. 里程碑與 Definition of Done

| Milestone | 交付物 | 必須通過的 gate |
|---|---|---|
| M0 契約凍結 | v1 proto/schema、artifact manifest、capability matrix、threat model | schema/property tests；不破壞 v0 proof vectors |
| M1 圖靈核心 | reference interpreter、supervisor、Minsky/recursion/heap/cancel fixtures | differential + fuzz；timeout/cancel 全部 kill/reap；hostile escape tests |
| M2 CPU 科學 | tensor ABI、dtype/complex、broadcast/reduce、BLAS/LAPACK、FFT、ODE、RNG、Monte Carlo、sparse | NumPy/SciPy/reference golden；誤差與 failure semantics gate |
| M3 Worker 接線 | runtime routing、CAS/chunk transfer、quota/telemetry、retry/idempotency | 多次重派不重複結算；大檔 resume；Worker shutdown/queue full/E2E |
| M4 GPU beta | CUDA/ROCm image、driver matrix、device artifact、CPU fallback | capability mismatch 不誤派；GPU/CPU 結果與成本標記正確 |
| M5 可用性發布 | 文件、SDK 範例、benchmark dashboard、support matrix、回滾方案 | reproducibility、security、performance、release image digest 全部簽核 |

每個 milestone 必須提交：測試命令與結果、fixture/hash 清單、benchmark 原始資料、已知限制、rollback 方法與明確 owner。沒有這五項，只能算 prototype，不能標示為 production-ready。

## 7. 效能、可重現性與營運門檻

- 先在固定 CI runner 建立 CPU、記憶體、artifact、cold-start、warm-start、BLAS、FFT、ODE 與 GPU baseline；後續 PR 若 p95 退化超過 20% 必須阻擋或附上核准的理由。
- warm sandbox 的 supervisor overhead 目標不超過同等直接 backend 執行時間的 15%；cold-start、image pull、CAS throughput 與 GPU initialization 分開報告，不能混成一個平均數。
- 預設 reproducible profile 固定 seed、thread count、BLAS reduction policy、CPU feature mask 與 image digest；best-effort profile 必須在結果中明示。
- 觀測至少包括 queue latency、startup、CPU/wall ratio、peak RSS、I/O、GPU time/VRAM、cancel/timeout、artifact retry、backend mismatch、reproducibility mismatch 與 unverified claim count。
- 發布前必須做長跑 soak、重派與節點故障、CAS 中斷續傳、同一任務重播、惡意輸入、超大 shape、NaN/Inf、fork/thread bomb 與 container escape 測試。

## 8. 建議實作順序（前十個 PR）

1. 凍結 `general-compute-v1` request/result、artifact manifest、runtime/image/cost-model version。
2. 建立 `general-compute-runtime` crate 的 process supervisor 與 bounded protocol，先不接 scheduler。
3. 先寫 Minsky machine 與 non-termination cancellation 的 RED tests，再接 Python backend。
4. 加入 Linux rootless sandbox、cgroup/seccomp/network deny、process group kill/reap。
5. 加入 artifact CAS、chunk checksum、size limits 與 resume tests。
6. 建立 CPU scientific image，鎖定 Python/NumPy/SciPy/BLAS/LAPACK/FFT 版本與 SBOM。
7. 實作 tensor envelope、dtype/shape/stride/view、NaN/Inf/complex 與 malformed-input tests。
8. 依 S2 順序加入 matmul/LU/QR/FFT/ODE/RNG/Monte Carlo/sparse golden fixtures。
9. 接 Worker runtime routing、telemetry、idempotency 與 Nodepool claim verification。
10. 在固定 runner 執行 benchmark/security/E2E gates，完成 M3 release review 後才開放 beta。

## 9. 當前狀態與下一個可執行動作

本次清理已移除未使用的 Monty CLI、舊 Docker server、Windows x86_64 helper、release 文件與不再需要的 workspace 成員；Hivemind 的 Docker、Windows package 與 config 不再提供 `MONTY_EXECUTABLE`。因 `executor-rs` 是獨立且有使用者 dirty modifications 的 repository，Monty 核心目錄及其相關 dirty 檔案暫不整批刪除，避免破壞未提交工作；它們已不再進入 Hivemind workspace、Docker 或部署路徑。

本輪驗證已通過：

- `cd executor-rs && cargo test --workspace`：29 tests passed。
- `cd hivemind-rs && cargo check -p hivemind-config`：passed。
- `cd hivemind-rs && cargo check -p hivemind-worker-executor`：passed。
- `scripts/docker-compose-release.Tests.ps1`：passed。
- `scripts/package-worker-windows.Tests.ps1`：passed。

下一個實作 checkpoint 是 M0：新增 v1 schema/manifest 的 RED tests 與 `general-compute-runtime` 空 crate；在 M0 完成前不修改 v0 語義、不接任意 Python 套件、不宣稱平台已圖靈完備。

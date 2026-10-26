# Managed Runtime 演進計畫：圖靈完備與科學運算

## 0. 先講結論

目前 active 的 `managed-function-runtime` 是 deterministic、bounded、可計量的
`hcode` JSON DSL。它適合做受 Nodepool consensus 保護的函式結算，
但不能誠實宣稱是「部署時圖靈完備」或「完整科學運算環境」：值域仍是有限的
整數、布林、字串、list、dict、null；每個 replica 以 `max_cpt` 作為工作額度，
並另外受 call-depth、value、output、materialization 與 deadline 等結構性限制。
歷史 `managed-function-v0` 的固定計量與結算契約已凍結，只保留相容性讀取與測試，
不再接受新的 v0 submission。

本計畫不把這些互相衝突的目標硬塞進同一個 managed runtime，而是採分層 runtime
契約：

| Runtime | 目的 | 宣稱 | 結算方式 |
|---|---|---|---|
| `hcode` | 小型 deterministic 函式、可計量的結算 | 有限 DSL；不宣稱圖靈完備 | Nodepool 以多 Worker consensus 與有效 replica usage evidence 結算 |
| `managed-function-v0` | 已完成任務與舊 Worker 的相容性 | 凍結的歷史有限 DSL；不接受新工作 | 保留原有歷史 fixed-reservation 語義，不轉換成 hcode |
| `general-compute-v1alpha1` | 一般程式與科學工作負載的 pre-release 契約 | 採 pinned CPython 的一般程式語義；單次執行仍受資源配額限制 | 僅允許 allowlisted beta；usage 先視為 claim，不直接驅動結算 |
| `general-compute-v1` | 通過 M0–M5 後的穩定契約 | 只宣稱 support matrix 與 gates 已證明的 CPU／GPU 能力 | 依 Nodepool 驗證出的 evidence level 結算 |

「圖靈完備」在這裡是語言的抽象語義宣稱，不是允許生產任務無限消耗 CPU 或記憶體。任何實際平台都必須有 CPU、記憶體、wall-time、取消、儲存與輸出上限；非終止程式應被可預期地取消，而不是拖垮 Worker。

## 1. 目標與不可接受的結果

### 目標

- 保持 v0 的 deterministic、bounded、可重播與 consensus result contract 相容性。
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
- 讓 v1 的巨大執行軌跡依賴單一 Worker 或單一執行結果作為結算依據。

## 2. 目標架構

### 2.1 契約分層

`hcode` 已是 active managed runtime；它沿用封閉 interpreter，並以每個
replica 的 usage allowance 與有效 evidence 進行 Nodepool settlement。`managed-function-v0`
的 runtime id、cost-model id 與 consensus result contract 仍是不可變的歷史邊界；新的
v0 submission 會被拒絕，未完成的 v0 work 在 Nodepool 啟動時取消，已完成資料只作
相容性讀取。只有完成 M0–M5 的 schema、tensor ABI、sandbox、verifiability、migration
與 release gates，`general-compute-v1alpha1` 才能升為 `general-compute-v1`。目前
程式碼中過早使用的 `general-compute-v1` 常數必須在 M0 改為 alpha id，避免未凍結契約
被誤認為穩定 API。

任何 managed 語義或計量改動都必須產生新 runtime／cost-model id、fixture 與 rollout，
不能原地修改 frozen v0。hcode 與完整 Python scientific backend 仍是不同契約，不應混為一談。

本計畫另明確區分執行邊界：active `hcode` 以及相容用的 frozen v0
interpreter 都只執行封閉自訂語法，沒有 filesystem、network、process、DLL 或 native
API capability。`production_sandboxed_dsl` 仍保留其 frozen v0 semantics identity，並在
自身路由中使用對應的封閉 interpreter；Windows DSL Worker 不需要 Windows
Containers/HCS。真正需要 operator-owned runner、image、rootfs、artifact mounts 或
外部程式的 general-compute workload，才分別使用 Linux `production_sandboxed_oci` 或
Windows `production_sandboxed_windows`/HCS，且各自的 provider prerequisite 不得套用到 DSL。

Rust control plane 負責驗證請求、建立 sandbox、套用配額、串流 artifact、取消／kill／reap、收集 telemetry 與產生結果 envelope；guest/backend 負責執行使用者程式。

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

- `executor-rs/crates/managed-function-runtime`：提供 active v1 DSL、canonical renderer、metering
  與共識結果語義，同時保留 frozen v0 compatibility fixtures 與 parser behavior。
- `executor-rs/crates/general-compute-runtime`：新增 request/result schema、supervisor、quota、
  cancellation、artifact、capability 與 backend adapter；不得依賴 Hivemind database。
- `hivemind-rs/crates/worker-executor`：新 managed work 只分派 v1；保留 v0 historical
  recognition，以及 detached supervisor 的 kill/reap cleanup guard。
- `proto/hivemind.proto`：只傳版本化 manifest、hash、狀態與受限 metadata；大型資料走 artifact service/CAS。
- `packaging/` 與 Docker/worker package：以 image digest、backend manifest、driver compatibility matrix 產生可重現包。

### 2.2 Runtime request/result 契約

`general-compute-v1alpha1` 的請求至少包含：

- immutable `execution_id`、`attempt_id`、idempotency key 與 request digest；重派不得重複結算。
- runtime／request schema／result schema／tensor ABI／billing／cost-model／evidence policy version。
- guest image digest、backend id、entrypoint、required typed capabilities、signed backend manifest／SBOM digest 與 source/package artifact digest。
- input manifest：大小、MIME/type、shape/dtype metadata、chunk hashes、整體 checksum。
- `ExecutionPolicy`：CPU quota、memory limit、wall-time、最大 process/thread、暫存空間、output bytes、network/filesystem/GPU capability、取消 deadline。
- `DeterminismPolicy`：seed、thread count、CPU feature set、reproducible／best-effort 模式。
- billing version 與 cost model version。

結果至少包含：

- `completed`、`failed`、`cancelled`、`timed_out`、`resource_exhausted`、`backend_unavailable` 等明確狀態。
- request digest、execution/attempt binding、exit/error code、受限 stdout/stderr preview、output artifact manifest root、output hash 與 checksum；大型 output 不得內嵌在 result frame。
- 實測 CPU time、wall time、peak memory、I/O bytes、GPU time/VRAM（皆標註為 worker claim，未驗證前不得結算）。
- runtime/backend/image/cost-model/result-schema/tensor-ABI 版本、input manifest root、seed/RNG algorithm/stream、實際 determinism profile 與 capability negotiation 結果。
- evidence envelope（`unverified`、`replicated`、`tee_attested`）；此欄由 Nodepool 驗證後衍生，Worker 不得自行決定 evidence level。

`GeneralComputeResult::validate_against(request, registry)` 是 M0 必須完成的可信邊界：驗證 unknown fields/版本、request/attempt binding、status 與 exit-code 合法組合、artifact role/size/root、usage 不超過 policy、實際 backend/image/determinism，以及 evidence 格式。安全敏感 envelope 採 `deny_unknown_fields` 或顯式 compatibility wrapper，不能讓新欄位被舊 verifier 靜默忽略。

### 2.3 建議 crate/module 邊界

- `contract.rs`：versioned request/result/evidence envelope 與 canonical request hash。
- `artifact.rs`：CAS reference、chunk/root manifest、commit/resume/idempotency validation。
- `tensor.rs`：dense/sparse ABI、checked shape/layout arithmetic 與 canonical logical hash。
- `backend.rs`：Nodepool-approved backend registry、signed manifest與 typed capability matching。
- `protocol.rs`：bounded framed/streaming protocol；source/input 不進 command line。
- `supervisor/{mod,linux,windows}.rs`：lifecycle 與平台隔離；public caller不能提交任意 host command。
- `reference.rs`：pinned CPython contract harness；不另造一個 Python interpreter。

## 3. 圖靈完備性工作包

### G0：定義語義與紅測試

以 pinned CPython 作語言語義與 reference backend，先寫 contract／formal fixtures，再接 sandbox backend；不為 v1 重寫 interpreter。必須驗證：

- 任意迴圈（`while` 或等價控制流）。
- 遞迴與至少可表達的函式／閉包呼叫。
- 可成長的 mutable heap、條件分支與狀態更新。
- 抽象語義中的任意精度整數；實際執行由 memory/time quota 終止。
- 明確的 exception、exit、cancel 與 resource-exhausted 語義。

驗收：兩計數器（Minsky machine）或等價 tape fixture可編碼並通過加法、遞減、零測試與 halt；BigInt、可成長 heap、recursion、exception具 reference fixtures；有限 quota 得到 typed `resource_exhausted`；非終止 fixture 在 deadline 內取消且 process tree已 kill/reap；direct pinned CPython與 sandbox backend做 differential tests。

### G1：核心 backend 與資源邊界

- 實作 `general-compute-runtime` supervisor 與版本化 wire protocol。
- 以 monotonic clock、cgroup/job object、process group、memory limit 與 output pipe cap 套用配額。
- cancellation 必須是 cooperative request 加 hard kill；任何 future drop、Worker shutdown、timeout 都必須完成 child reap 後才釋放 concurrency slot。
- stdin/stdout/stderr 都有上限與 back-pressure；錯誤不得把 source、input 或 secret 放進 command line 或公開 log。
- parser、loader、ABI decoder 與 protocol decoder 加入 fuzz target。
- `CommandSpec` 不成為 untrusted public API；只有 registry-approved backend可構造命令。Linux production採 rootless OCI、cgroup v2、PID/mount/user/network namespaces、seccomp、`no_new_privs`、read-only root、明確 mounts與 subreaper/init。Windows只有在 container/Hyper-V isolation與Linux policy等價後才進 support matrix；Job Object或 `taskkill /T` 單獨不足。
- total stdout+stderr preview共用上限；超限依 policy終止或轉 artifact，不能只截斷 retained bytes卻無限 drain hostile output。future drop、normal leader exit但 descendant持 pipe、timeout與cancel都必須有 drop guard/kill/reap coverage。

驗收：100% 的 timeout/cancel/child-exit path 沒有 orphan process、hung task 或 semaphore leak；同一 input/image/seed 在 reproducible profile 重跑得到相同 output hash；hostile workload 不能突破 memory、wall-time、output、filesystem 或 network policy。

## 4. 科學運算工作包

### S1：數值型別與資料 ABI

定義 versioned binary tensor envelope，不用 JSON 表示大型數值資料：

- scalar：`float32`、`float64`、`complex64`、`complex128`、signed/unsigned integer、arbitrary precision integer；明確支援 IEEE-754 NaN、`+Inf`、`-Inf`、signed zero 與 byte order。
- 第一版 ndarray metadata：`abi_version`、`dtype`、shape、byte order、contiguous C/F layout、data artifact與 canonical logical hash。非連續／負 stride view先在 sandbox邊界 materialize；只有完成 checked reachable-byte-range與 canonical hash規格後才加入 signed stride/offset。
- 互通：CPU 先支援 NumPy `.npy/.npz` 或等價 versioned envelope；程序內優先採 Arrow C Data Interface，device buffer 採 DLPack；避免未驗證的 pickle。
- sparse：CSR、CSC、COO 的 index dtype/base、shape、sorted/duplicate policy、indptr/indices/data checksum與結構 invariant必須寫入 manifest。
- chunked artifact：每 chunk 有 hash，整體有 Merkle/root checksum，支援 resume、range fetch、CAS dedup 與大小上限。

驗收：空陣列、零維陣列、shape-product/offset overflow、NaN/Inf/signed zero/subnormal、complex、BigInt sign/magnitude scalar、endian conversion、稀疏空列與 malformed metadata 都有 golden tests；禁止 pickle/object dtype；native、sandbox、artifact round-trip 的 metadata 與 logical hash一致。非連續/負 stride則先驗證 materialization結果，不能在 ABI 尚未定義時宣稱 zero-copy支援。

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

- Elementwise與穩定 scalar運算以 `|actual-reference| <= atol + rtol*|reference|` 驗證；ULP-sensitive cases另用 ULP gate。容忍度按 dtype、維度、演算法與 condition number定義，不用一個全域 `1e-12` 冒充所有科學正確性。
- LU/QR/solve 額外檢查 residual、orthogonality 或 backward error；不能只比較最後幾個數字。
- FFT round-trip、ODE known solution、Monte Carlo confidence coverage 與 sparse reference fixtures 必須在 CI 通過。
- ill-conditioned、overflow、underflow、NaN/Inf propagation 與 singular matrix 必須有明確 status，不能靜默產生成功結果。
- RNG契約固定 algorithm、seed-sequence version、stream/subsequence id與parallel splitting policy；只記一個 seed不足以跨版本重播。

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
- registry-approved image/capability與 Worker自報 capability是不同資料；Nodepool須記錄 claim、persisted registration、operator-approved registry與attested capability的 provenance。字串相符或 `gpu_available=true` 不是硬體存在的證明。
- active `hcode` 由 Nodepool 以 strict-majority consensus certificate 選出
  結果，並獨立以驗證過的 per-replica usage evidence 結算；有效但 divergent 的 replica
  也可按實際工作量結算，未使用的 hold 會退回。`general-compute-v1alpha1` 的 resource
  telemetry 仍一律存為 `worker_usage_claim`，不得單獨驅動 variable settlement；其
  fixed reservation 會維持到對應 evidence policy 完成驗證。Replicated output 只能提高
  結果可信度，不能單獨證明 CPU/GPU usage。
- evidence level 由 Nodepool 在驗證後寫入；Worker 只能附 evidence bytes，不能宣告自己是 `tee_attested`。
- 每個版本同時鎖定 runtime version、cost model、result/consensus protocol、trust policy 與 golden fixtures；任一項變更都產生新版本。

## 6. 里程碑與 Definition of Done

| Milestone | 交付物 | 必須通過的 gate |
|---|---|---|
| M0a 凍結 v0 | semantics/cost manifest、既知 Unicode/overflow/partial-result限制、consensus result vectors | v0 result fixtures不漂移；公開文件不誇大 |
| M0b v1alpha 契約 | IDs、request/result/evidence validator、artifact/tensor schema、typed capability、threat model | property/fuzz/replay/unknown-field/shape-overflow tests；runtime id仍為 alpha |
| M1 圖靈核心與 sandbox | pinned CPython harness、trusted backend registry、rootless supervisor、Minsky/recursion/heap/cancel fixtures | differential + fuzz；timeout/cancel/drop/leader-exit全部 kill/reap；hostile escape tests |
| M2 CPU 科學 | tensor ABI、dtype/complex、broadcast/reduce、BLAS/LAPACK、FFT、ODE、RNG、Monte Carlo、sparse | NumPy/SciPy/reference golden；誤差與 failure semantics gate |
| M3 Worker/Nodepool/CAS 接線 | typed runtime routing、attempt binding、CAS/chunk transfer、quota/telemetry、evidence/idempotency | forged hardware/image/output/usage/evidence/replay皆拒絕；重派不重複結算；大檔 resume；多節點 E2E |
| M4 GPU beta | CUDA/ROCm image、driver matrix、device artifact、CPU fallback | capability mismatch 不誤派；GPU/CPU 結果與成本標記正確 |
| M5 可用性發布 | canary/migration、文件、SDK 範例、benchmark dashboard、signed image/SBOM、support matrix、回滾 | reproducibility、security、performance、compatibility 全簽核後才把 id升為 `general-compute-v1` |

M3 trusted capability registry gate 已落地：Nodepool operator config 是 worker general-compute capability snapshot 的唯一來源；registration 以 owner binding 寫入 Postgres，untrusted heartbeat 不得覆蓋 snapshot，owner-authorized registration 可撤銷 snapshot，scheduler 對 `general-compute-v1alpha1` 僅依 persisted snapshot 與 request matching 做 admission。Attempt-bound request/result compatibility、inline artifact materialization、reference-only typed backend execution、bounded Worker typed-result RPC wiring、immutable Nodepool artifact repository、generation-bound Prepare/resume/chunk transfer 與 completion 後的完整 envelope persistence 已各自完成小 checkpoint；Nodepool 現在會在 completion 前驗證完整 typed result envelope。剩餘 gate 包含 Nodepool-owned production input-digest validation、真實 rootless OCI execution/isolation、多進程 E2E 與 trusted usage/billing settlement。

每個 milestone 必須提交：測試命令與結果、fixture/hash 清單、benchmark 原始資料、已知限制、rollback 方法與明確 owner。沒有這五項，只能算 prototype，不能標示為 production-ready。

## 7. 效能、可重現性與營運門檻

- 先在固定 CI runner 建立 CPU、記憶體、artifact、cold-start、warm-start、BLAS、FFT、ODE 與 GPU baseline；後續 PR 若 p95 退化超過 20% 必須阻擋或附上核准的理由。
- warm sandbox 的 supervisor overhead 目標不超過同等直接 backend 執行時間的 15%；cold-start、image pull、CAS throughput 與 GPU initialization 分開報告，不能混成一個平均數。
- `strict_reproducible_cpu` 固定 architecture/features、rounding、單 thread、BLAS/FFT與 reduction policy；`reproducible_same_profile`只承諾同 image/hardware profile；GPU/parallel CPU預設 `best_effort`並用數值 acceptance rules，不宣稱跨硬體 bitwise一致。
- 觀測至少包括 queue latency、startup、CPU/wall ratio、peak RSS、I/O、GPU time/VRAM、cancel/timeout、artifact retry、backend mismatch、reproducibility mismatch 與 unverified claim count。
- 發布前必須做長跑 soak、重派與節點故障、CAS 中斷續傳、同一任務重播、惡意輸入、超大 shape、NaN/Inf、fork/thread bomb 與 container escape 測試。
- benchmark保存 raw data與 p50/p95：sandbox cold/warm start、cancel latency、CAS upload/download/checksum/resume/dedup、elementwise/reduction、DGEMM多尺寸、FFT、SpMV、ODE、Monte Carlo、CPU thread scaling、GPU init/transfer/kernel/fallback；active v1 native與replicated consensus path是不可退化 baseline，v0 僅作歷史相容性 regression evidence。

## 8. 建議實作順序（前十個 PR）

1. 凍結 v0 semantics/cost manifest與既知限制；把現有新契約改名 `general-compute-v1alpha1`。
2. 補齊 execution/attempt IDs、result/evidence validation、typed capabilities、canonical request hash與 replay tests。
3. 完成 artifact/CAS與第一版 contiguous tensor ABI的 property/fuzz tests。
4. 將 process supervisor藏在 trusted backend registry後，加入 rootless OCI、cgroup/seccomp/network deny、subreaper/drop kill/reap。
5. 加入 artifact CAS、chunk checksum、size limits 與 resume tests。
6. 建立 signed CPU scientific image，鎖定 Python/NumPy/SciPy/BLAS/LAPACK/FFT 版本、digest與 SBOM。
7. 實作 tensor envelope、dtype/shape/stride/view、NaN/Inf/complex 與 malformed-input tests。
8. 依 S2 順序加入 matmul/LU/QR/FFT/ODE/RNG/Monte Carlo/sparse golden fixtures。
9. 接 Worker runtime routing、attempt-bound telemetry/evidence、idempotency與 Nodepool verification；unverified usage不做 variable settlement。
10. 在固定 runner 執行 benchmark/security/E2E gates，完成 M3 release review 後才開放 beta。

## 9. 當前狀態與下一個可執行動作

清理 commit `be39bb7` 已移除未使用的 Monty CLI、舊 Docker server、Windows x86_64 helper、release 文件與不再需要的 workspace members；Hivemind 的 Docker、Windows package與 config不再提供 `MONTY_EXECUTABLE`。本輪另以 Cargo target graph證明並移除重複、未編譯的 `hivemind-rs/crates/hivemind-bin/src/main.rs`。

`executor-rs` 現在只保留 Hivemind 的 `managed-function-runtime` 與
`general-compute-runtime`。未接線的 Monty core、CLI、JS/Python bindings、typeshed、fuzz
crate，以及其專用 Makefile／CI／IDE metadata 已在使用者明確授權後移除；這些內容不再是
Hivemind 的 source、build 或 release surface。

`general-compute-runtime` 已有 contracts、capability validation、framed JSON、bounded supervisor
與 output capture；這仍是 M0/M1 scaffold，尚未完成完整 production sandbox、CAS、tensor ABI、
scientific image 或可信 billing。Master、Nodepool、Worker 現在接受 active v1 managed path；
舊 v0 只保留歷史資料與相容解析，新的 v0 work 會被拒絕。

已保存的 cleanup／scaffold驗證包括：

- `cd executor-rs && cargo test --workspace`：29 tests passed。
- `cd hivemind-rs && cargo check -p hivemind-config`：passed。
- `cd hivemind-rs && cargo check -p hivemind-worker-executor`：passed。
- `scripts/docker-compose-release.Tests.ps1`：passed。
- `scripts/package-worker-windows.Tests.ps1`：passed。

下一個實作 checkpoint 是 attempt-bound request/result compatibility；M0a/M0b contract、artifact/tensor canonical hashing 與 M3 alpha transport/admission 已完成，但這些 gate完成前仍不接任意 Python套件、不做 variable usage settlement、不宣稱平台已具 general/scientific compute。


### M3 alpha transport/admission checkpoint (2026-08-13)

Commit 8b34285 transports validated general-compute-v1alpha1 request-manifest bytes through HTTP Master, Master/Nodepool gRPC, Postgres task persistence, scheduler dispatch, and Worker admission. The Nodepool explicitly rejects a manifest on any other runtime and rejects legacy torrent input for the alpha runtime; the Worker separately checks the operator-owned backend/image capability allowlists. A successfully admitted alpha request still returns UNIMPLEMENTED until backend execution, artifact materialization, CAS transfer/resume, attempt-bound result validation, and Nodepool settlement evidence are implemented. Verification: Master API 29, Nodepool 69, scheduler 81 passed with 1 intentional external-verifier ignore, proto 3, worker admission 7, plus offline integration cargo check for scheduler, Master API, Nodepool, worker executor, and binary.

### M3 trusted capability registry checkpoint (2026-08-13)

The Nodepool trusted registry gate is now implemented. Operator configuration is the only source for a worker's general-compute capability snapshot; registration binds the configured worker id to the authenticated owner (or admin) and rejects mismatches with `PermissionDenied`. Snapshots are persisted in `worker_nodes.general_compute_capabilities_json`, survive untrusted heartbeat refreshes, and can be explicitly revoked by an owner-authorized registration. Scheduler admission for `general-compute-v1alpha1` parses only the persisted Nodepool snapshot and validates backend, image, capability, thread, network, filesystem, and GPU requirements against the request. Missing or malformed snapshots fail closed. The next checkpoint is attempt-bound request/result compatibility, then backend execution and CAS materialization.

### M3 attempt-bound request/result compatibility checkpoint (2026-08-13)

The alpha dispatch contract now carries immutable execution/attempt/idempotency/request-digest identity through the Worker RPC. Worker alpha responses echo the identity on success and failure, while legacy managed-function responses remain empty. Nodepool validates identity against the persisted request before completion; mismatches redispatch without settlement. Retry resets rotate only `attempt_id` and recompute the canonical request digest, preserving execution and idempotency identity. The repository completion guard compares the exact persisted manifest so stale attempts cannot settle directly. Verification: scheduler lib 89 passed/1 intentional ignored, DB-backed retry/stale-result/manifest-guard tests passed, and locked scheduler/Worker/proto checks passed. Worker test linking remains subject to the existing Windows MSVC/MinGW mixed-linker symbol; Worker library check is green.

### M3 fixed-reservation settlement checkpoint (2026-08-14)

Nodepool completion now persists a typed settlement provenance row for `general-compute-v1alpha1`. The row binds the accepted execution/attempt/idempotency/request-digest identity, approved `billing-v1` and `cost-v1` versions, the Worker usage claim, and Nodepool-derived `unverified` evidence. Alpha billing remains a fixed Nodepool reservation (`max_cpt`); usage claims are retained for audit and policy validation but cannot select variable pricing. Completion fails closed on malformed or mismatched result identity, non-completed status, non-unverified Worker evidence, non-canonical output artifacts, policy-exceeding usage, or an unapproved billing/cost version. Multi-process/container OCI E2E and operator deployment validation remain open, so this is not a production-readiness declaration.

The Worker routing fixture now covers task-specific bundle materialization through an operator-owned pinned fake runner and catches canonical artifact-root spelling drift on Windows. It remains a process-level contract test only; real rootless OCI/container isolation and a Postgres-backed multi-process completion run are still required release gates.

### GPU request-level contract checkpoint (2026-08-14)

Commit `6400099` binds the typed `GpuRequirement` to `ExecutionPolicy` with
fail-closed `gpu_required` consistency validation while preserving omission of
the optional field in the default CPU request JSON. RED→GREEN evidence includes
focused request tests (2), the full contracts suite (21), locked runtime serial
and four-thread suites, runtime check, and GNU Worker/Task Scheduler/Bin checks.
This completes only request validation; trusted Nodepool registration, selected
device identity/result binding, actual CUDA/ROCm execution, OCI E2E, and trusted
usage/billing settlement remain open.

### Operator-owned Compose deployment boundary checkpoint (2026-08-14)

The release Compose contract now exposes the production general-compute
registry through the fixed in-container path
`/etc/hivemind/general-compute/backends.json` on a named read-only volume, and
keeps task bundles, materialized artifacts, and the durable CAS journal under a
separate mutable state volume. The registry entries must carry absolute
operator-owned bundle/rootfs/artifact paths; Compose does not infer host paths,
and an absent registry still fails production routing closed. RED→GREEN coverage
in `scripts/docker-compose-release.Tests.ps1` verifies the source contract and
the resolved `docker compose config --format json` mount modes. This is only an
operator packaging boundary; real rootless OCI/container E2E and trusted
variable usage/billing settlement remain release gates.

### Scheduler trusted GPU result identity checkpoint (2026-08-14)

The scheduler now consumes the operator-owned typed GPU registration and
requires the Worker result's `GpuSelection` to equal the deterministic trusted
selection for the persisted request. The forged-device RED test is green, and
the GNU scheduler library gate passes (118 passed, 1 ignored); commit
`5be0e48 feat(scheduler): verify trusted GPU result identity` is local and not
pushed. This is an
authoritative comparison at result admission, not hardware attestation or GPU
execution; Worker device discovery, CUDA/ROCm, OCI E2E, operator deployment,
and trusted usage/billing settlement remain open.

### Worker trusted GPU selection checkpoint (2026-08-14)

Commit `0052444 feat(worker): bind trusted GPU selection to results` wires the
operator-owned `TrustedWorkerCapabilityRegistration` through Worker admission
and reference execution. Typed GPU requests fail closed without a compatible
approved identity, and deterministic `GpuSelection` is bound into successful
and failure `GeneralComputeResult` envelopes while CPU JSON stays compatible.
Focused runtime GPU (1), Worker admission (2), GNU Worker test compile, and
staged diff checks pass. This remains claim-level plumbing; CUDA/ROCm driver
execution, hardware attestation, real OCI/container E2E, operator deployment,
and trusted usage/billing settlement are still required.

### Trusted GPU registration checkpoint (2026-08-14)

Commit `b22aaab` extends the operator-owned worker registration with typed
`GpuCapability` identities and a fail-closed deterministic selector. Legacy
registration JSON without the optional list remains readable as an empty list.
The focused registration suite (3 tests), locked runtime suites, and GNU
cross-crate checks pass. Scheduler/Worker admission and result identity binding
are the next integration gate; this does not claim hardware attestation or
CUDA/ROCm execution.

### GPU result identity checkpoint (2026-08-14)

Commit `67bc1c9` binds the selected typed GPU identity to
`GeneralComputeResult`. CPU results omit the optional field for JSON
compatibility. GPU-required results now fail closed unless they carry a
requirement-compatible `GpuCapability` or an explicitly requested CPU
fallback; `validate_against` checks the vendor/runtime/driver/VRAM/stream/image
identity before accepting the result contract. Focused result tests (3), the
locked runtime serial/four-thread suites, runtime check, and GNU Worker/Task
Scheduler/Bin checks pass. This is still a claim-level runtime contract:
trusted Nodepool comparison against the operator-owned registration,
scheduler/Worker admission wiring, actual CUDA/ROCm execution, OCI E2E, and
usage/billing settlement remain open.

### OCI E2E startup-guard checkpoint (2026-08-14)

The reviewed multi-process OCI fixture now retries Master authentication while
Nodepool gRPC becomes ready (`6166bff`) and the harness rejects a missing,
relative, or non-regular canonical case-plan file before Compose startup
(`34a91a5`). OCI harness, Compose release, runtime workspace, and diff-check
gates pass. The complete M3/M4/M5 release gate is still open until an operator
provides the pinned registry/rootfs/runner/seccomp material and case plan, then
produces evidence for real rootless OCI isolation, typed result persistence,
timeout/cancel cleanup, hostile workload denial, and Nodepool settlement.

### Typed cancellation-result checkpoint (2026-08-14)

Commit `f7495a3` closes the Nodepool persistence gap for accepted
general-compute cancellation. Task status and the typed cancelled result are
written in one transaction; the envelope is request/attempt/backend/image
bound, uses canonical inline-input identity or a domain-separated immutable
manifest-coordinate digest when CAS bytes were never materialized, and creates
no settlement. DB-backed scheduler cancellation tests (4) and the Nodepool
stop-task compatibility test pass on the current locked workspace. Real OCI
timeout/cancel kill-reap evidence still requires operator-provided assets and
the canonical case plan, so the overall M3/M4/M5 release gate remains open.

### Typed stale-timeout result checkpoint (2026-08-14)

Commit `0ec476c` makes the scheduler's stale-`RUNNING` sweep persist a typed
Nodepool timeout result in the same transaction as the `TIMED_OUT` task state.
The result is `timed_out`/`worker_heartbeat_lost`, is bound to the immutable
request and backend/image identity, uses canonical inline-input identity or a
timeout-specific domain-separated manifest-coordinate digest for unmaterialized
CAS inputs, and creates no settlement. The DB regression was observed RED with
`RowNotFound` and is now green (1/1); cancellation regressions (4/4), locked
Scheduler/Nodepool checks, and Nodepool stop-task compatibility (1/1) also pass.
This closes the typed persistence boundary only; real OCI timeout kill/reap,
host isolation, operator deployment, and trusted settlement remain required.

### Typed Nodepool failure-result checkpoint (2026-08-14)

Commit `f186b4b` makes `fail_for_worker` atomically persist a Nodepool-generated
typed `failed`/`nodepool_task_failed` envelope for general-compute tasks, which
covers the HEAD-present max-redispatch terminal path. The envelope remains
request/backend/image bound and non-settling; existing Worker reputation and
rejected-attestation behavior is preserved. The DB test was observed RED with
`RowNotFound` and is now green (1/1); focused failure compatibility (2/2),
consensus-only settlement rejection (1/1), scoped formatting/diff checks, and locked
Scheduler/Nodepool/Master checks pass. Real OCI execution/isolation and the
operator-gated multi-process evidence remain open.

### Guarded generic failure-result checkpoint (2026-08-14)

Commit `c10b803` makes the public generic fail path terminal-safe and typed.
Only active task states can transition to `FAILED`; a completed task can no
longer be overwritten. General-compute task state and the Nodepool-generated
`failed`/`nodepool_task_failed` envelope are persisted atomically without a
settlement. Separate RED tests captured the missing result (`RowNotFound`) and
terminal overwrite, and both are green; scoped format/diff and locked
Scheduler/Nodepool/Master compatibility checks pass. Remaining M3 work includes
the dirty CAS/immutable-artifact regressions and operator-gated OCI evidence.

### Durable CAS transfer-state checkpoint (2026-08-14)

Commit `ec44b65` persists immutable execution/artifact transfer state below the
operator CAS root so a Worker restart or rotated attempt can resume verified
chunks without allowing manifest redefinition. Create-new transfer manifests
and completion markers fail closed on corruption and reconcile only from
rehash-verified CAS objects. A clean-HEAD test-only probe was RED on the absent
API; focused transfer tests are 4/4, the exact staged artifact suite is 9/9,
the integrated runtime suite passes, and Worker/Scheduler/Bin compatibility
checks pass. The current dirty Nodepool CAS slice is now scheduler-green
(124 passed, 1 ignored), but its repository and dispatcher layers still need
to be isolated into focused local commits before M3 CAS/resume can be claimed
complete.

### General-compute settlement schema recovery checkpoint (2026-08-14)

Commit `9f1d332` restores the `general_compute_settlements` migration consumed
by the committed typed terminal-result paths. Four DB-backed terminal tests
first failed because the relation was absent; the focused migration and all
four cancellation/timeout/failure regressions now pass. This restores the
existing fixed-reservation provenance boundary only and does not establish
variable usage settlement or production OCI evidence.

### Nodepool immutable artifact repository checkpoint (2026-08-14)

Commit `a13804b` persists task-bound artifact identity, immutable chunk
coordinates, verified inline sources, uploaded chunks, completeness, and
expiry. Task creation is atomic with manifest validation; source reads rehash
content and fail closed on metadata drift; chunk retries are manifest-bound
and byte-idempotent; chunk content and the completeness transition commit in
one row-locked transaction. Focused repository tests pass 22/22, Database
11/11, the validation-overlay Scheduler gate passes 107 tests with 1
intentional ignore, and exact-commit Scheduler/Worker/Bin production checks
pass. The next isolated unit is Nodepool transfer-lease lifecycle, followed by
dispatcher preparation/chunk RPC; operator-gated OCI and trusted variable
settlement gates remain open.

### Nodepool transfer-lease lifecycle checkpoint (2026-08-14)

Commit `5b22af8` persists a Nodepool-owned, monotonically generated transfer
lease for each general-compute assignment. Assignment and claim atomically bind
the lease to task/execution/attempt/Worker identity; redispatch and all committed
terminal transitions revoke it, expiry is materialized, assignment drift fails
closed, and legacy tasks receive no lease. Focused lease tests pass 5/5,
Scheduler passes 113 with 1 intentional ignore, Database passes 12/12, and the
Scheduler/Worker/Node Manager/Master/Bin production and validation-overlay test
checks pass. The broader dirty transport slice remains Scheduler-green at 130
passed with 1 intentional ignore after adding its no-penalty terminal revoke.
The next isolated unit is the authenticated token/protobuf/Nodepool gRPC lease
authority, followed by Worker enforcement and dispatcher chunk preparation;
real rootless OCI and trusted variable settlement remain release gates.

### Managed-runtime lockfile compatibility checkpoint (2026-08-14)

Commit `b3abec8` repairs the committed lockfile's stale
`managed-function-runtime` 0.0.7 entry to match the already committed 0.1.0
workspace manifest. The clean RED was Cargo refusing `--locked --offline`
before compilation; after deterministic offline regeneration, Scheduler,
Worker, Node Manager, Master API, and Bin all pass locked offline checks. This
build-gate repair is separate from the lease feature and does not change the
next functional unit: authenticated Nodepool lease authority.

### Worker execution-token transfer identity checkpoint (2026-08-14)

Commit `b22fed5` extends Nodepool-issued Ed25519 Worker execution tokens with
execution/attempt/idempotency/request-digest identity and a positive transfer
generation. Invalid identities are rejected before signing, while legacy base
claims remain decodable for managed-function compatibility. The missing API and
invalid-identity acceptance were each observed RED before implementation. Auth
passes 7/7 with strict clippy and scoped format checks; five production
consumers pass locked offline checks. Next isolate the protobuf lease-validation
RPC contract, then implement Nodepool database authority and Worker enforcement.

### Bounded transfer-lease authority envelope checkpoint (2026-08-14)

Commit `bae0207` freezes the bounded Worker-to-Nodepool lease-authority request
and active/rejected response. The request binds the Nodepool-issued Ed25519
execution token to task, Worker, execution, attempt, idempotency, canonical
request digest, and positive generation; the shared validator enforces an 8 KiB
token cap, 255-byte identity caps, valid SHA-256 syntax, and a 16 KiB encoded
message cap. Compile and behavioral RED tests preceded the implementation; the
isolated Proto suite passes 12/12, scoped format/clippy gates pass, and five
production consumers pass locked offline checks. The next isolated unit adds
the RPC method and Postgres-backed Nodepool token/active-lease authority; Worker
enforcement remains a separate following unit.

### Nodepool transfer-lease authority checkpoint (2026-08-15)

Commits `f017606`, `b7d8e34`, `a9d2e35`, and `acc173a` independently refresh
the test fixtures required by the evolved backend/capability schema. Commit
`ecbbee4` then registers `ValidateGeneralComputeTransferLease` and makes
Nodepool the live authority: the shared bounded validator runs before token or
database work, the Ed25519 claims must match every request identity field, and
the trusted repository must return the same active assignment/generation after
materializing expiry and terminal/assignment drift. The real tonic/Postgres
test covers valid authority, invalid token, all identity drifts, revocation,
reassignment, attempt/generation rotation, replacement authority, and expiry.
Isolated and integrated Scheduler, Proto, Auth, Worker, Node Manager, Master API,
and Bin gates pass; the integrated Scheduler result is 130 passed with 1
intentional environment-gated ignore. The next isolated unit is Worker
production fail-closed enforcement for Prepare/Upload/Resume, followed by
dispatcher authenticated chunk preparation. This checkpoint is not rootless
OCI isolation, multi-process hostile-workload evidence, or trusted variable
settlement.

### Generation-bound chunk wire-contract checkpoint (2026-08-15)

Commit `cacd0eb` binds chunk upload and resume to a positive Nodepool transfer
generation and defines bounded Prepare request/response messages carrying the
full lease identity. Compile RED tests preceded the minimal contract change;
Proto passes 13/13 in the isolated worktree, focused GNU Worker chunk tests and
Worker test-target compilation pass after four fixture-only compatibility
updates, and Scheduler, Node Manager, Master API, and Bin pass locked/offline
consumer checks. Integrated dirty-main Proto is 14/14 and focused Worker chunk
verification is green. The service RPC and enforcement are deliberately the
next TDD unit: Worker must consult Nodepool and fail closed before mutating
prepared state or CAS for Prepare/Upload/Resume, with no production local-allow
fallback. Dispatcher transfer and the remaining OCI/settlement gates follow.

### Worker production transfer-authority checkpoint (2026-08-15)

Commit `df48f19` makes Worker chunk preparation fail closed against the trusted
Nodepool lease authority. `PrepareGeneralCompute`, upload, and resume bind the
Nodepool-issued token to task/Worker/execution/attempt/idempotency/request-digest
identity plus a positive generation, revalidate authority before prepared-state
or CAS mutation, and distinguish denial from authority unavailability. The
production binary must inject the real Nodepool client; the old no-authority
constructor is protected by a compile-fail regression. RED compile/behavioral
tests preceded the implementation. Integrated gates pass Proto 15/15, Worker
GNU 107/107 plus chunk 9/9, GPU 2/2, admission 7/7 and doctest 1/1, real
Postgres-backed Nodepool authority 2/2, Scheduler 130 with 1 intentional ignore,
and locked checks for all five consumers. Next isolate the dirty dispatcher
authenticated preparation/source-transfer path into its own TDD commit. This
checkpoint does not prove rootless OCI isolation, hostile-workload E2E, operator
deployment assets, or trusted variable settlement.

### Dispatcher authenticated source-transfer checkpoint (2026-08-15)

Commit `94576b6` completes the Nodepool dispatcher side of the authenticated
transfer sequence. Before Worker execution it obtains the active assignment
lease, signs task/Worker/execution/attempt/idempotency/request-digest/generation
identity once, loads and rehashes immutable Nodepool source rows, always sends
the bounded Prepare envelope, resumes per artifact, and uploads only Worker-
reported descriptors that exactly match the Nodepool manifest. Inline manifest
bytes never replace repository authority in production.

The error paths are also bounded: missing, expired, or mismatched leases and
transport failures rotate the attempt for no-penalty redispatch; signing or
missing trusted source creates a typed Nodepool failure, revokes the lease, and
creates neither settlement nor Worker reputation/attestation penalty. Compile
and behavioral REDs preceded the implementation. Commit-local Scheduler is 125
passed with 1 intentional ignore; integrated dirty-main Scheduler is 142 passed
with 1 intentional ignore, Proto is 15/15, and Worker/Node Manager/Master API/
Bin all-target checks pass. The commit was advanced locally with an empty index
and no push. Next isolate production OCI input-digest verification against the
same Nodepool-owned materialized bytes; real rootless OCI/hostile-workload E2E,
operator assets, and trusted variable settlement remain release gates.

### Distinct-owner replica selection checkpoint (2026-10-21)

Phase 0 of the consensus hardening plan. Commit `f62f4bfc` replaces the greedy
ranking by self-reported status and CPU usage with a uniform draw over admitted
Workers, at most one replica per provider account. Commit `d763a34f` adds
`MANAGED_CONSENSUS_REQUIRE_DISTINCT_OWNERS` (default `true`). Setting it to
`false` removes the owner rule, logs a warning, and is for local smoke and E2E
runs only.

RED→GREEN evidence: disabling the owner dedupe made
`select_replicas_never_repeats_an_owner` fail at seed 0 with "repeated an owner".
The config tests did not compile before the field existed. After `f62f4bfc`,
task-scheduler passed 183 tests against `hivemind_test`. The workspace run passed
821 tests at `f62f4bfc` and 827 at `d763a34f`, with 0 failed and 0 ignored.

The owner rule is keyed on the provider account. One operator that registers
several accounts is not covered. Closing that gap needs registration controls
outside the scheduler.

### Lazy third replica checkpoint (2026-10-21, partial)

Phase 1 of the consensus cost work. Commit `a16c1feb` pins the existing 2-of-3
certificate behavior with a test-only change: two agreeing observations certify,
while a disagreement or a single observation does not. Commit `2255b79f`
dispatches a quorum of two for enforce-mode hcode. It adds the third replica only
when the replicas in flight cannot reach quorum, or when the deadline passes
without a certificate. Escalation extends the deadline once, capped at twice the
timeout from creation. The usage hold still covers `replica_count` replicas, and
an undispatched replica's allowance is refunded at settlement. Observe mode and
`production_sandboxed_dsl` still assign every replica.

RED→GREEN evidence: the new DB tests did not compile before the implementation
existed. The first behavioral run caught a defect. The timeout test showed the
escalation refused because the old deadline had just passed, so the repository
now checks the extended deadline. The same run exposed a fixture gap: a timed-out
replica's stop could not confirm, so the fake Worker now marks dropped executions
finished. GREEN: task-scheduler passed 195 tests against `hivemind_test`. The
workspace run passed 753 tests across the binaries that reported. The
`hivemind-client-runtime` library hung in
`update::tests::activation_rolls_back_when_replacement_process_cannot_start`
under full-workspace load and passed 85 of 85 when run alone. That hang predates
this work.

Unfinished in Phase 1:

- T1.4 (replica count, p50/p95 completion latency, and CPT billing before and
  after): not run. It needs a live Master, Nodepool, and at least three Workers,
  plus a baseline build of the pre-change commit. No before-and-after numbers
  exist yet.
- T1.3 event logging is emitted from the same path as T1.2, so it has no separate
  commit. The escalation test and its secret-absence assertion cover its
  acceptance items.
- Escalation after a Nodepool restart rebuilds the spare pool from the registered
  Workers. No test exercises that path yet.
- Expected replica counts depend on disagreement and timeout rates, which have
  not been measured.

### T1.4 benchmark checkpoint (2026-10-21)

Local before/after run of the lazy third replica (`2255b79f`, docs at
`0e6427d4`) against the pre-change baseline `a7ac5c25`, on one Windows host.
These numbers are non-production and indicative only. This checkpoint closes
the T1.4 item listed as not run above.

Environment: ASUSTeK ASUSPRO D840MB_M840MB; Intel Core i7-8700 3.20 GHz, 6 cores
and 12 threads; 31.9 GB RAM; Windows 11 Education 22631; PowerShell 5.1.22621;
rustc and cargo 1.98.1; PostgreSQL 18.3 and Redis on localhost. Both builds use
the debug profile with `CARGO_BUILD_JOBS=1`. Binary SHA-256 values are in the
raw `environment.txt`: base `AA2585E0…`, head `DB124657…`. Consensus settings
are identical: enforce mode, 3 replicas, quorum 2, 30 s timeout, and
`MANAGED_CONSENSUS_REQUIRE_DISTINCT_OWNERS=true` (the baseline ignores it).

Setup: one Nodepool, one Master, and three Workers, each under a different
provider account. Each Worker is reached through a fault proxy in one of three
modes: `normal`; `wrong` (a wrong result with a valid digest); `timeout` (the
reply is held for 35 s). Each scenario sets one faulty Worker. The main campaign
runs 3 repetitions per scenario with 20 tasks per run, base and head
interleaved. The stress set runs 2 repetitions of the normal path with 60 tasks
per run. Each run starts from an idle queue.

Metrics: exec/task counts distinct replica IDs that reached a Worker, from the
proxy log. worker-s/task sums proxy forward durations and excludes held replies.
billed is `tasks.billed_amount`. Latency is DB `created_at` to `completed_at`
over completed tasks. Failed tasks have no `completed_at`, so latency percentiles
exclude them. p95 is nearest-rank.

Main campaign (3 × 20 tasks per cell):

| build | scenario | completed | exec/task | worker-s/task | billed CPT | median s | p95 s | timed-out replicas/task | escalated |
|---|---|---|---|---|---|---|---|---|---|
| base | normal | 100.0% | 2.95 | 0.08 | 346.9 | 2.05 | 5.43 | 0.00 | 0.0% |
| head | normal | 75.0% | 2.00 | 0.03 | 242.0 | 3.51 | 4.23 | 0.00 | 0.0% |
| base | wrong | 100.0% | 3.00 | 0.05 | 363.0 | 3.32 | 5.04 | 0.00 | 0.0% |
| head | wrong | 73.3% | 2.65 | 0.05 | 320.7 | 4.40 | 10.57 | 0.00 | 65.0% |
| base | timeout | 100.0% | 3.00 | 0.05 | 242.0 | 9.79 | 19.69 | 1.00 | 0.0% |
| head | timeout | 88.3% | 2.63 | 0.04 | 240.0 | 30.17 | 35.07 | 0.63 | 63.3% |

Stress (normal path, 2 × 60 tasks per build):

| build | completed | failed | deadlock log lines | exec/task | median s | p95 s |
|---|---|---|---|---|---|---|
| base | 100.0% | 0 | 0 | 2.97 | 2.46 | 2.83 |
| head | 78.3% | 26 | 26 (13 per run) | 2.00 | 15.53 | 25.43 |

Findings:

1. Replica execution and charge. Head executes fewer replicas in every
   scenario. On the normal path it executes 2.00 instead of 2.95 and bills 242
   CPT instead of 347 (−30%). With a wrong result the gap is smaller (2.65 vs
   3.00; 321 vs 363 CPT). With a timeout it is nearly zero, because the
   timed-out replica is not billed in either build.

2. Timeout latency is clearly worse: median 30.2 s vs 9.8 s, p95 35.1 s vs 19.7
   s. A third replica is dispatched only after the 30 s deadline passes without
   quorum, so a held replica costs the full window. This follows from the
   escalation design. Nothing was changed.

3. Wrong-result p95 is worse (10.6 s vs 5.0 s), but the sample is small: about
   45 completed tasks per build. The per-run p95 ranges overlap (4.5–5.1 s), so
   the gap is not separable at this size.

4. Burst latency is clearly worse: stress p95 25.4 s vs 2.8 s. In the slowest
   tasks the replies arrived about 0.05 s after dispatch, but dispatch itself was
   delayed by about 30 s. The Nodepool log shows repeated `consensus Worker …
   has no available execution slot` warnings, roughly every 5 s, before the
   attempt was dispatched. Head logs 84–98 such warnings per stress run. The
   baseline logs none in the same burst. The slot check is the same code in both
   builds (`task_repository.rs:2222` in base; `ensure_consensus_worker_slot` in
   head). One candidate cause is that reservations from stuck attempts count
   against capacity until their deadline. That is not verified, and the first
   warning came before the first deadlock, so deadlocks do not explain the whole
   gap.

5. Settlement deadlock. This is the blocking problem.
   - All 64 failed head tasks share one signature: `FAILED`, `managed consensus
     deadline exceeded`, `managed_consensus_state = no_quorum`, hold settled, and
     121–363 CPT billed. Each stress run's Nodepool log holds 13 `deadlock
     detected` lines and 13 `Managed consensus execution failed` warnings, one per
     failed task. The main runs have no log slice, so their cause is inferred from
     the matching DB signature.
   - The PostgreSQL log shows two transactions, each running `UPDATE users SET
     balance = balance + $1 ... WHERE username = $2 AND is_active = true AND
     balance <= $3`, each waiting on the other's row lock. This is the provider
     credit loop in settlement. It walks payouts sorted by `replica_id`
     (`task_repository.rs:781`; the baseline sorts the same way at `:719`). What
     differs is the mapping from `replica-N` to provider account. Uniform
     selection (`f62f4bfc`) can place replica-1 and replica-2 on different
     accounts from one task to the next, so two settlements can lock the same two
     accounts in opposite order. The baseline ranking kept that mapping mostly
     stable. This is a hypothesis and has not been reproduced in isolation.
   - When settlement deadlocks after a certificate has formed, the task ends
     `FAILED`, but the requester is still charged for accepted usage. The path is
     `settle_managed_v1_hold_without_certificate` with reason `no_quorum`
     (`task_repository.rs:2981`). The contract allows usage-only settlement, but a
     billed failed task is a product decision, not only a bug.
   - The baseline completed 100% of 180 main tasks and 120 stress tasks, and its
     stress logs contain no deadlock lines.

6. Selection. Replicas are drawn uniformly at random from Workers that pass the
   hard filters (status, `queue_capacity`, memory), one per provider account. No
   weighting was added. The degradations above are reported as measured.

Cost-model decision (I2): not applied. It needs a decision.

- Grep result. The replica count appears in `HIVEMIND_MANUAL.zh-TW.md` §8.2
  (`reserved_usage = R × a`, example `R = 3`) and in `docs/HCODE_RUNTIME.md`
  ("three distinct Workers by default"). It does not appear in the frozen manifest
  `hcode-semantics.json` or in its digest `1fb0e736…`. The manifest records only
  per-replica allowance vocabulary and has no replica or quorum count. Cost
  vectors, per-replica budget derivation, the fee rule, and settlement inputs are
  unchanged.
- Under the rule, the count appears in a cost-model document and the case is
  undecidable, so the bump applies. It is not a one-string edit. The manifest's
  `change_policy` is `immutable`, and a replacement publishes new runtime,
  cost-model, and manifest identifiers. The runtime identifier is `hcode`, which
  clients submit, so the change needs a drain and a migration. Editing the
  cost-model ID in place would break that policy. Not applied here.
- Decision needed: either publish a v2 identity (new runtime, cost-model, and
  manifest identifiers, with a coordinated upgrade), or record an explicit
  exception for this dispatch change. Until one is chosen, `2255b79f` should not
  be deployed.
- Fee change, documented in `docs/HCODE_RUNTIME.md` and manual §8.2: a normal
  task is charged for two replicas. The third is charged only after
  disagreement, timeout, or deadline. The hold still covers every configured
  replica, and undispatched allowances are refunded.

Known flaky test (pre-existing, not fixed):
`update::tests::activation_rolls_back_when_replacement_process_cannot_start` in
`hivemind-client-runtime`. On `a7ac5c25`, ten solo runs each hit the 300 s cap.
The lazy-third checkpoint recorded the library's 85 tests passing when the
library was run alone. Why the filtered solo run of this one test stalls while
the library run passes is not established. Workspace runs skip the test with
`--skip activation_rolls_back_when_replacement_process_cannot_start`.

Workspace run on the `0e6427d4` source (`b48b80ee` changed only docs since):
`hivemind-rs` workspace, debug profile, `-j 1`, `HIVEMIND_REQUIRE_TEST_DATABASE=1`
against `hivemind_test`, `--skip activation_rolls_back_when_replacement_process_cannot_start`.
Exit code 0. 41 test binaries reported 837 passed, 0 failed, 0 ignored. One test
was filtered out, the skipped one. The binary that holds it ran 84 of 85 tests.
Database-backed tests ran against the database, because the require flag turns
a missing database into a failure. The `executor-rs` workspace was not run as a
whole in this checkpoint.

Limitations:

- Debug builds. Both builds report `CpuScore 0` (the smoke script's setting), so
  placement has no CPU term. `benchmark.ps1` reached a ready CPU state, but
  scoring needs a release build.
- Worker-seconds sit near the proxy's resolution (0.03–0.13 s per task).
- Samples are small and variance is high. The base normal per-run median ranges
  from 1.98 to 5.14 s.
- Nodepool, Master, Workers, proxies, and PostgreSQL share one host.
- `scripts/hivemind-smoke-benchmark.ps1` sends `task_source` as `{"value": …}`
  under Windows PowerShell 5.1, because `Get-Content -Raw` adds note properties.
  Every submission then returns 422. The bench used a copy that reads the files
  with `ReadAllText`. The repository script is unchanged.
- The benchmark Nodepool sets `HIVEMIND_ADMIN_USERS` to the requester so the
  smoke script can list Workers. Admin status controls listing and admin routes,
  not scheduling or settlement.

Open items:

- Settlement lock order. Make the credit order deterministic across tasks, for
  example by provider account name, and add a two-settlement concurrency test.
  Fix before `2255b79f` is deployed.
- Billing for failed tasks. Decide whether usage-only settlement applies when a
  task ends `FAILED`.
- Burst slot waits. Instrument per-Worker slot occupancy, then decide whether
  stuck attempts should release reservations earlier.
- I2 decision (above).
- Raw CSV, JSON, and log slices are in `C:\hm-bench\results` and are not
  committed.

### Settlement lock-order and deadlock retry checkpoint (2026-10-21)

Commit `27231242`. The deadlock reported above is reproduced by
`concurrent_settlements_with_reversed_provider_order_do_not_deadlock`. A holder
transaction pins one provider account, so the first settlement queues on it and
the second queues behind the first. Each settlement takes its provider accounts in
replica order, the two form a cycle, and PostgreSQL aborts one of them with
SQLSTATE 40P01.

RED: the test fails with a deadlock at `a7ac5c25`, the earliest commit tried, at
`f62f4bfc` (T0.1), and at `8ad89641`, the head before this change. The defect
predates T0.1 and T1.2. The baseline's ranking kept the bench's replica-to-provider
mapping stable, which is why the baseline showed no deadlocks. Random selection
makes the reversed mapping common. The settlement code is unchanged since the
baseline.

Fix: `lock_user_accounts` in the database crate takes the row locks of every
account whose balance a transaction changes, in ascending username order, before
the first balance update. It is applied to the managed v1 usage-only settlement,
the certified settlement, the legacy managed completion, the general-compute
completion (the provider lookup moved ahead of the debit), and user transfers.
The GPU completion already locked its owner and provider in sorted order and was
left as it was.

Retry: the certified settlement, the legacy managed completion, the no-quorum
finalization, the stop-pending finalization, the GPU completion, and the
general-compute completion retry a transaction that fails with 40P01, at most three
attempts in total. Each attempt starts a new transaction. The retry is insurance;
the ordering removes the cycle. User transfers have no retry.

GREEN: the regression test passes three times in a row at `27231242`. The
task-scheduler suite passes (196 tests) with the database attached.

### Certified expired-quorum guard checkpoint (2026-10-21)

Commit `61d9226d`. The expiry path finalized a stopped enforce attempt as
no-quorum without checking whether its persisted replies already formed a
certificate. When the certified settlement failed, the usage-only no-quorum path
settled the hold and failed the task, charging the requester for accepted replica
usage. All 64 failed head tasks in the T1.4 run had this shape.

`settle_persisted_quorum` rebuilds the certificate from the persisted replies. If
one exists, it settles it. A failed settlement leaves the attempt active, and the
next expiry pass retries it. Neither the expiry path nor the in-loop no-quorum path
finalizes such an attempt. A quorum that never settles stays active, and each pass
logs a warning. This is deliberate: the task is not failed and the requester is not
charged.

The same commit fixes `persisted_managed_consensus_observation`. It compared the
stored digest with the digest of the full result bytes, but the dispatcher stores
the canonical digest. Every reply that carried usage was rejected, so startup
recovery ignored all persisted replies. Recovery now resumes from them. This changes
restart behavior and is covered by the same commit.

RED: `expired_quorum_with_failed_settlement_stays_active_and_unbilled` injects the
failure at the certificate insert, which only the certified path performs. With the
guard disabled it fails with "a certified quorum must not be finalized". With the
guard it passes.

### initial_replicas switch checkpoint (2026-10-21)

Commit `d027a125`. `MANAGED_CONSENSUS_INITIAL_REPLICAS` sets how many replicas an
enforce-mode hcode attempt dispatches up front. Unset dispatches all
`MANAGED_CONSENSUS_REPLICA_COUNT` replicas, which is the behavior before lazy
dispatch existed. A value from quorum up to the replica count enables lazy dispatch.

Separability. Each T1.2 change was checked against `a16c1feb`, the commit before it:

- Dispatch count: read from the config now. Spares are created only when the
  initial count is below the replica count, as before.
- Early exit when quorum is unreachable: the one loop change that affected a fully
  dispatched attempt. The pre-T1.2 loop waited for in-flight replicas. The exit is
  now limited to lazily dispatched attempts.
- Settlement count checks (plan, usage-only, certified, stop-pending finalization):
  a fully dispatched attempt always has exactly `replica_count` replica rows, so the
  relaxed checks accept and reject the same attempts as the strict ones.
- Evidence classification existed before T1.2.
- Vote recording, the stop and drain sequence, and observation handling match the
  pre-T1.2 loop.

The switch therefore separates without a schema change. The equivalence rests on the
test suite and this review, not on a trace comparison.

Tests: task-scheduler 197 pass, config 37 pass, the workspace checks, clippy with
`-D warnings` on the changed crates, and rustfmt.

Documentation: the contract and manual were returned to their previous text
(`dce1bd76` reverts `b48b80ee`), so the cost-model documents describe the default
unchanged. The opt-in is documented in `managed-consensus-state.md`, the README, the
getting-started guide, the public limitations, the smoke benchmark guide, and
`.env.example` (`12b25406`).

### Cost-model decision (I2), resolved

The replica count still appears in cost-model documentation (manual §8.2 and
`HCODE_RUNTIME.md`). No identifier was bumped: `hcode-execution-cpt-v1` and the
manifest digest are unchanged. The switch keeps the default dispatch identical, so
the documented default stays accurate. A v2 identity waits for a real buyer.

### v2 design memo (deferred)

Not part of this change. Recorded so the decisions are not lost.

- Cost-model v2 (`hcode-execution-cpt-v2`): needs new runtime, cost-model, and
  manifest identifiers under the manifest's `change_policy`, a drain, and a
  migration. Only with a real buyer.
- Default flip of `initial_replicas` to quorum: wait until the burst slot-wait and
  timeout-latency items below are settled.
- Failed-task billing: a certified quorum whose settlement fails now stays active
  and is not billed. A task without a certificate still settles accepted usage
  through the usage-only path, so a task that times out or disagrees can end FAILED
  and be billed. Decide whether a FAILED task should be billed at all.
- Burst slot waits: lazy and full dispatch both wait on Worker slots. Held replicas
  keep their reservations until their stop is confirmed, which in the timeout case
  is the deadline. The T1.4 rerun checkpoint located the waits. Any change needs a
  decision on when a held replica frees its slot.
- Timeout escalation latency: after a timeout the third replica waits for the full
  deadline. Earlier escalation is a design change and is not applied.
- Selection: uniform random over the hard filters stays. No weighting.

### Status of the open items above

- Settlement lock order: fixed in `27231242`. The legacy managed completion
  retries on 40P01 as well, in `ace74498`.
- Billing for failed tasks: changed on 2026-10-26 to A2 under
  `hcode-billing-v3-failed-no-charge` (see the decisions below).
- Burst slot waits: located by the rerun checkpoint below; not changed.

### T1.4 rerun checkpoint (2026-10-21, release)

Commit `ace74498`, which contains the settlement lock order, the certified-quorum
guard, the `initial_replicas` switch, and the legacy deadlock retry. Both variants
run the same release binary (`hivemind-bin.exe`, SHA-256 `51E1B553…05A9`). They
differ only in `MANAGED_CONSENSUS_INITIAL_REPLICAS`: `reloff` leaves it unset
(eager, all replicas dispatched) and `relon` sets it to 2 (lazy). The replicas per
normal task confirm the switch: 3.00 for `reloff`, 2.00 for `relon`.

Setup matches the T1.4 checkpoint above: one host, one Nodepool, one Master, three
Workers behind fault proxies, 20 tasks per scenario run, three repetitions with the
variants interleaved, and a 60-task burst on the normal path. Each variant and
repetition starts a fresh stack. The first campaign used debug builds, so absolute
latencies are not comparable with it. Compare `relon` with `reloff` only.

Main campaign, pooled over 3 × 20 tasks per cell:

| build | scenario | completed % | exec/task | billed CPT/task | median s | p95 s | escalated % |
|---|---|---|---|---|---|---|---|
| relon | normal | 100.0 | 2.00 | 242.0 | 0.83 | 1.04 | 0.0 |
| relon | wrong | 100.0 | 2.67 | 322.7 | 4.61 | 5.02 | 66.7 |
| relon | timeout | 98.3 | 2.58 | 240.0 | 30.09 | 34.98 | 58.3 |
| reloff | normal | 100.0 | 2.95 | 350.9 | 4.65 | 5.37 | 0.0 |
| reloff | wrong | 100.0 | 3.00 | 361.0 | 4.89 | 4.98 | 0.0 |
| reloff | timeout | 100.0 | 3.00 | 242.0 | 9.61 | 19.55 | 0.0 |

Burst, normal path, 2 × 60 tasks per variant:

| build | completed % | exec/task | median s | p95 s | slot-wait warnings | deadlock lines |
|---|---|---|---|---|---|---|
| relon | 100.0 | 2.00 | 4.28 | 4.47 | 0 | 0 |
| reloff | 100.0 | 2.99 | 4.85 | 5.12 | 0 | 0 |

Findings:

1. Replicas and charge. On the normal path, lazy dispatch runs 2 replicas instead
   of 3 and bills 242 CPT per task instead of 351 (−31%). With a wrong result the
   charge drops from 361 to 323 CPT (−11%). With a timeout it is nearly unchanged
   (240 against 242 CPT).

2. Normal-path latency. In the main campaign the medians differ: 0.83 s for `relon`
   against 4.65 s for `reloff`. The per-run ranges do not overlap (0.47–0.99 s and
   1.52–5.32 s). The burst shows a much smaller gap (4.28 s against 4.85 s). The
   cause is not established.

3. Timeout latency. `relon` has median 30.1 s and p95 35.0 s; `reloff` has median
   9.6 s and p95 19.6 s. These match the shape of the first campaign's timeout
   results for head and base. Escalation waits for the deadline before the spare is
   dispatched. Not changed in this round.

4. One FAILED task in `relon` timeout (1 of 60; `managed consensus deadline
   exceeded`, state `no_quorum`, 121 CPT billed). Trace for
   `smoke-20261021-212316-w3-t20-2`, as offsets from the first dispatch:
   - replica-1 on bench-w2 replies normally.
   - replica-2 on bench-w1 (timeout mode) holds its reply. At +30.005 s the Nodepool
     marks it failed (`Timeout expired`) and escalates by adding replica-3.
   - The stop for replica-3 reaches bench-w3 at +30.011 s. Its `ExecuteTask` reaches
     bench-w3 at +30.029 s and replies at +30.031 s. The Nodepool does not count the
     reply, and the attempt ends `no_quorum` at its 60 s deadline.
   - Other timeout tasks in the same runs take the same escalation and form a
     certificate with `escalations=1`. The reason for the early stop is not
     identified.

5. Slot waits in timeout runs. Warnings appear only in the timeout scenario:
   `relon` has 10 lines (9 in rep 1, 1 in rep 3) and `reloff` has 48 (16 in each
   rep). The normal and wrong scenarios and the burst have none. A temporary build
   that dumps the occupants at each refusal was used for diagnosis. It was not
   committed, and its patch is kept outside the repository. The dump shows:
   - Every refusal is on `bench-w1`, the timeout Worker. Its capacity is 12, the
     CPU count the Worker reports, and all 12 slots are held by attempts in state
     `running`.
   - Reservations are released only when an attempt is finalized after all stops
     are confirmed. In the lazy runs the occupants had a median age of 5–15 s and a
     maximum of about 30 s, which is the deadline stop. In the eager runs the
     maximum was about 5 s.
   - Peak concurrency from the reservation table: the eager runs reach 12 on all
     three Workers. The lazy runs reach 12 on `bench-w1` and 10 on `bench-w2` and
     `bench-w3`. The logs record refusals only on `bench-w1`. Why `bench-w2` and
     `bench-w3` never log a refusal is not established. Dispatch order is a
     candidate.
   - Interpretation: a held replica keeps its reservation until its stop is
     confirmed, so a 20-task burst fills a 12-slot Worker. Changing this means
     deciding whether a held replica's slot can be freed before its stop is
     confirmed. That is a design decision and was not made here.

6. Submission limit (bench side). The first attempt at burst rep 102 was rejected
   entirely with HTTP 429. The Master limits each owner to 60 task submissions per
   minute per process (`HIVEMIND_TASK_SUBMIT_LIMIT_PER_MINUTE`, default 60). Rep 101
   used the full allowance on the same stack, so rep 102 must start on a fresh
   stack. The rejected runs are kept in `results\invalid-429` and are not part of
   the figures above. The rerun on fresh stacks is the one reported.

7. No run has deadlock lines or proxy stop failures.

Limitations: one host, release builds, three repetitions per cell, so small samples.
The held replica is simulated by a proxy, so the Worker itself finishes quickly.
Per-run medians vary widely (for example, `reloff` normal from 1.5 to 5.3 s).

Open items after this checkpoint:

- Ordering of the escalated replica's stop (finding 4).
- When a held replica frees its slot (finding 5). This is a design decision.
- Timeout escalation latency (finding 3).
- Billing for a FAILED task: the timeout task in finding 4 was billed 121 CPT.
- Late receipts after a confirmed stop: a stop-pending attempt with a persisted late
  quorum still finalizes as `no_quorum` with usage billing (test
  `managed_v1_stop_pending_keeps_hold_until_confirmed_then_pays_valid_usage`).
  Policy decision pending.
- Default flip of `initial_replicas` remains deferred (v2 memo).

### Decisions on the open items (2026-10-26)

Each item is decided against the documented contract where one exists, and against
outside practice otherwise. Changing a decision is a product call; the reasons are
recorded so that a change can be made deliberately.

- **Slot release while a stop is unconfirmed.** Keep. The reservation stays until
  the stop is confirmed, as `managed-consensus-state.md` requires. Releasing it
  early can overlap a replica the Worker is still running. Kubernetes also waits
  out a toleration window (300 s by default) before evicting pods from a NotReady
  node.
- **Replies after a stop.** Keep the fence. A reply recorded while the attempt is
  `stop_pending` is kept as usage evidence. It cannot complete the task, and under
  the current identity it is charged only if the task settles with a certificate.
  A reply that reaches a finalized attempt is rejected:
  no vote and no billing (`managed_consensus_no_quorum_fences_late_observations`).
  BOINC (arXiv 1903.01699) checks a copy that arrives after a canonical result
  against it to decide credit, without changing the result. Our rule has the same
  shape. An earlier draft of this entry said valid usage is billed without this
  distinction; it was too broad.
- **Billing for FAILED tasks without a certificate.** Changed on 2026-10-26, as
  approved by the project owner: A2. The requester pays nothing, and providers are
  not paid for receipts on a task without a certificate. New holds use
  `hcode-billing-v3-failed-no-charge` with `hcode-execution-cpt-v2`; earlier holds
  keep their rule. A stop-pending attempt that finalizes as `no_quorum` after a late
  quorum follows the same rule (test
  `managed_v1_stop_pending_keeps_hold_until_confirmed_then_refunds_without_certificate`).
  Forum reports say BOINC pays by default only for results it can
  check. AWS Lambda bills executions that error; that is from a third-party guide,
  and the official pricing text was not read. A1 (platform-funded pay for valid
  receipts) stays available if provider attrition appears.
- **Timeout escalation latency.** Keep for this round, as instructed. v2 target: a
  percentile-triggered spare, opt-in first. The spare starts once the outstanding
  replica exceeds the p95 of observed reply times. The deadline stays the hard
  limit, and the rest are cancelled once a quorum forms. Tail at Scale uses this
  shape (secondary sources). Spark speculation uses 1.5 times the median with a
  0.75 quantile gate. A hedge adds a replica, about 121 CPT per task (half of the
  242 CPT two-replica normal task), so the budget needs a decision.
- **Lazy dispatch as the default.** Keep eager as the default. Lazy timeouts are
  still about three times slower (median 30.1 s against 9.6 s), and they hold
  slots until the deadline. The 31% normal-path saving stays available as opt-in.
  Revisit after the v2 hedge.
- **Cost-model identifier bump.** Done for the FAILED-task rule only:
  `hcode-execution-cpt-v2` is used by new holds. The runtime and manifest
  identifiers did not change, because the parser, limits, and receipt meaning are
  the same. No drain was needed, since the identity is recorded on each hold.
- **Relaxed replica-count checks in settlement.** Keep for now. Prerequisite for
  flipping the default: persist `initial_replicas` on the attempt and make the
  checks strict.
- **Retry path test gap.** Closed for the helper. `retry_on_deadlock` has unit tests
  in `d3693444`. The end-to-end deadlock path still relies on the lock-order test.
- **Expiry race after an escalation.** Fixed in `d3693444`. Regression evidence is
  recorded below once the run completes.
- **Codex review.** Not run. Codex is not logged in, and logging in needs the user.

### Expiry claim regression (2026-10-26)

Release build of `d3693444` (SHA-256 `ACFD76E9…C704D9`), bench build `fix`.
Timeout scenario only, 20 tasks per run, a fresh stack per run. The eager variant
leaves `MANAGED_CONSENSUS_INITIAL_REPLICAS` unset. The lazy variant sets it to 2.

| variant | runs | completed | median s per run | p95 s per run | replicas/task | billed CPT/task | `superseded` lines | escalation lines | stuck tasks |
|---|---|---|---|---|---|---|---|---|---|
| eager | 3 × 20 | 60/60 | 6.46, 6.42, 6.61 | 16.24, 16.29, 16.32 | 3.00 | 242 | 0 | 0 | 0 |
| lazy | 3 × 20 | 60/60 | 31.08, 31.43, 31.72 | 31.19, 31.54, 31.80 | 2.60 | 242 | 0 | 12 per run | 0 |

Findings:

- No regression. The eager runs fall inside the range of the earlier timeout
  runs (median 5.2–9.6 s per run). The lazy median stays near the 30 s deadline,
  which is the escalation-latency item (plan B).
- No task stays in a non-terminal state. No run has a failed task or an
  execution-failed log line.
- The claim refusal never fired (`superseded` is 0). The race reproduced once in
  60 lazy timeout tasks during the T1.4 rerun, so six runs cannot show the fix in
  the bench. The evidence for the fix is the RED test
  `expiry_leaves_attempt_whose_persisted_deadline_was_extended`, which fails on the
  previous expiry and passes now.

### Opt-in hedge and strict replica-row checkpoint (2026-10-26)

Commits: `568213a8` persists `initial_replicas` and `added_replicas` and checks the
replica rows strictly at settlement. `9a1f3d32` adds `MANAGED_CONSENSUS_HEDGE_AFTER_SECS`,
which is off by default. `9d26afcc` documents both. The benchmark binary is the release
build of `9a1f3d32` (SHA-256 `53A55F22…AE67`), bench build `hedge`. Each run starts a
fresh stack and has 20 tasks.

| variant | runs | median s per run | p95 s per run | replicas/task | billed CPT/task | hedge lines | slot warnings |
|---|---|---|---|---|---|---|---|
| eager default, timeout | 3 | 10.11, 5.98, 10.14 | 19.88, 15.62, 19.89 | 3.00 | 242 | 0 | 16 (run 601 only) |
| lazy, hedge 3 s, timeout | 3 | 12.42, 11.39, 12.77 | 12.50, 13.23, 22.57 | 2.6–2.7 | 242 | 12, 12, 14 | 2, 4, 6 |
| lazy, hedge 3 s, normal | 3 | 1.17, 1.44, 1.09 | 1.26, 1.48, 1.20 | 2.00 | 242 | 0 | 0 |

All nine runs complete every task, with no stuck or failed task.

Findings:

1. The hedge works as designed. The spare is added 3.0 s after dispatch. On the
   answering Worker it replies in about 1 ms. Hedges appear only on timeout tasks.
   The normal path has none, and billing is unchanged at 242 CPT.
2. The 3–5 s target was not reached. Lazy timeout median fell from about 31 s to
   11–13 s. The decomposition of one hedged task (`…w3-t20-1`) is:
   - create to dispatch: 4.4 s. Burst tasks wait for the dispatcher, and slot
     refusals add retries. The timeout runs have a median of 3.5–5.1 s, against
     1.1–1.4 s on the normal path.
   - hedge: 3.0 s, as configured.
   - quorum to certificate: 5.0 s. After quorum, `drain_managed_consensus_workers`
     waits a fixed 5 s for in-flight replicas before it records the certificate.
     The held replica's reply never arrives in that window. The same 5 s drain
     explains the eager timeout median of about 6 s, against about 1 s when all
     replicas reply.
3. The default is unchanged. Eager timeouts have a median of 6–10 s across runs,
   within the range of earlier runs. The replica count and billing match.
4. The normal-path median is 1.1–1.4 s. The earlier lazy normal runs were 0.5–1.0 s.
   The strict replica-row check adds one indexed read at settlement. Run-to-run
   variance is the more likely explanation, but it was not isolated.
5. Slot warnings in timeout runs fell to 2–6 per run, from 10 in the T1.4 relon runs
   and 48 in the T1.4 reloff runs. The normal path has none.

Decisions needed before the 3–5 s target can be met:

- The 5 s post-quorum drain. It exists to record usage receipts from stopped replicas
  that are still finishing. Options: keep it; make the grace configurable with the
  default unchanged, so an operator can trade late receipts for latency; or shorten
  the default, which changes settlement inputs and needs the identity gate.
- Create-to-dispatch delay under bursts. A task refused for a Worker slot stays
  pending until a later dispatch pass, so burst tasks wait for passes. The refusal
  is logged as `no available execution slot`. This is a throughput question,
  separate from the hedge.

### Drain window control checkpoint (2026-10-26)

Commits: `5185d040` makes the post-quorum drain configurable, with
`MANAGED_CONSENSUS_DRAIN_SECS` (`drain_secs`). The default stays 5 s, and the value
must not exceed the timeout. `2b30a1c5` documents it. The benchmark binary is the
release build of `5185d040` (SHA-256 `BF17E1EC…67E8`), bench build `drain`, timeout
scenario, 20 tasks per run, a fresh stack per run.

| variant | runs | median s per run | p95 s per run | dispatch→certificate | create→dispatch median | slot warnings | replicas/task | billed CPT/task |
|---|---|---|---|---|---|---|---|---|
| eager, drain 5 (default) | 3 | 9.80, 5.32, 5.52 | 19.58, 15.09, 15.29 | 5.02–5.03 s | 4.78, 0.26, 0.49 s | 14, 13, 16 | 3.00 | 242 |
| eager, drain 1 | 3 | 1.95, 1.55, 2.14 | 6.69, 6.23, 6.87 | 1.03 s | 0.92, 0.52, 1.10 s | 6, 6, 7 | 3.00 | 242 |
| lazy, hedge 3 s, drain 5 (default) | 3 | 8.56, 8.59, 8.62 | 18.40, 18.41, 18.40 | 8.02–8.03 s (hedge→certificate 5.02 s) | 0.53, 0.63, 0.62 s | 8, 8, 4 | 2.70–2.80 | 242 |
| lazy, hedge 3 s, drain 1 | 3 | 5.25, 4.61, 9.01 | 10.04, 4.84, 9.26 | 4.02–4.03 s (hedge→certificate 1.02–1.03 s) | 1.24, 0.70, 5.13 s | 3, 0, 0 | 2.55–2.70 | 242 |

All twelve runs complete every task, with no stuck or failed task.

Findings:

1. The knob does what it says. The certificate follows the last needed reply by the
   configured drain: 5.02 s at the default and 1.03 s at 1 s, for eager and lazy alike.
2. Eager timeouts drop from a median of 5.3–9.8 s to 1.6–2.1 s, and p95 drops from
   15–20 s to 6–7 s. Slot warnings roughly halve, from 13–16 to 6–7 per run. The
   drain holds each completed attempt's reservations for its whole length, so this
   is the same effect that produced the slot waits.
3. Lazy with hedge drops from a median of 8.6 s to 4.6–5.3 s in two of three runs.
   The third run, at 9.0 s, has a create-to-dispatch median of 5.1 s, so dispatch
   wait dominates there. Create-to-dispatch varies widely between runs: 0.5–5.1 s
   medians here, and 3.5–5.1 s in the earlier hedge runs, which also had a median of
   11–13 s with the same 5 s drain. The variation is not explained yet.
4. Billing is 242 CPT per task in every variant. The held replica never returns inside
   the window in this bench, so no receipt is lost. The bench cannot measure how often
   real late receipts arrive after 1 s, so the billing effect of a shorter window is
   unknown outside this bench.

Limitations: three runs per variant, one host, and a proxy that holds the held
replica's reply for 35 s, so late receipts are never tested here.

Decision: the default stays 5 s. An operator who prefers latency sets
`MANAGED_CONSENSUS_DRAIN_SECS=1` and accepts that a receipt arriving after one
second is not billed. Before changing the default, measure late-receipt arrival times
in a real deployment. The create-to-dispatch variation is the next open item; it needs
its own investigation before any change.

### Dispatch wakeup checkpoint (2026-10-26)

Root cause of the create-to-dispatch variation: the dispatch loop runs on a fixed
5 s tick (`hivemind-bin/src/lib.rs`, `start_registered_dispatch_loop(5s)`). A burst
created at once waits for the next tick, so its delay depends on where the burst
lands in the tick. Tasks refused for a Worker slot also waited a full tick before
retrying. The slow run 713 shows one pass, `Dispatched 20 pending tasks`, five seconds
after the burst.

Change: `6bd76c48` wakes the dispatch loop when a pending task is created and when an
attempt finishes, since a finished attempt releases its slots. `3ed92bff` coalesces
wakeups over 20 ms and documents the one-loop-per-process assumption. The 5 s tick
remains the fallback. The independent review of `6bd76c48` (through Codex) is handled
as follows: the wakeup cannot be lost; the per-completion scan is bounded by the 20 ms
coalescing; the unbiased shutdown select is kept, since a biased select would starve
dispatch if the watch sender were dropped; the process-global notification is
documented, not restructured.

The benchmark binary is the release build of `3ed92bff` (SHA-256 `2CFB67D4…9BBD`), bench
build `wakeup`, twelve runs of 20 tasks, a fresh stack per run.

| variant | runs | median s per run | p95 s per run | create→dispatch median | dispatch→certificate | slot warnings | billed CPT/task |
|---|---|---|---|---|---|---|---|
| lazy, normal path, no hedge | 3 | 0.24, 0.10, 0.17 | 0.29, 0.13, 0.21 | 0.22, 0.07, 0.12 s | n/a | 0 | 242 |
| lazy, hedge 3 s, drain 5 s (default) | 3 | 8.08, 8.09, 8.14 | 8.14, 8.20, 16.09 | 0.05–0.14 s (max 8.1 s in run 813) | 8.0 s (hedge→certificate 5.0 s) | 0, 4, 19 | 242 |
| lazy, hedge 3 s, drain 1 s | 3 | 4.10, 4.07, 4.17 | 4.22, 4.13, 4.27 | 0.06–0.18 s (max 3.9–4.0 s in runs 822, 823) | 4.0 s (hedge→certificate 1.0 s) | 10, 6, 5 | 242 |
| eager, drain 5 s (default) | 3 | 5.19, 5.18, 5.26 | 10.08, 10.08, 10.20 | 0.12–0.21 s (max 5.0–5.2 s) | 5.0 s | 34, 31, 24 | 242 |

All twelve runs complete every task, with no stuck or failed task.

Findings:

1. The variation is gone. Create-to-dispatch medians are 0.05–0.22 s, down from
   0.5–5.1 s. The normal path falls from 1.1–1.4 s to 0.10–0.24 s.
2. Lazy with hedge and a 1 s drain reaches a median of 4.07–4.17 s with p95 within
   0.15 s of it, so the 3–5 s target from the drain checkpoint is met. The remaining
   term is the 3 s hedge plus the 1 s drain, as designed.
3. Lazy with hedge and the default 5 s drain falls to 8.1 s, as expected from the
   3 s hedge plus the 5 s drain. Run 813 has one burst task that waited 8.1 s for a
   slot, which lifts its p95 to 16 s.
4. Eager timeouts keep their 5 s median, set by the drain, but p95 falls from 15–20 s
   to about 10 s.
5. Slot warnings rise for eager, from 13–16 to 24–34 per run. Dispatch now starts
   the whole burst at once instead of in a later batch, so more attempts compete for
   slots together, and refused tasks retry as soon as an attempt ends. The longest
   create-to-dispatch waits, about 5 s, match the drain, since slots free only when
   an attempt finishes its drain. This is the trade-off of starting sooner.
6. Billing is 242 CPT per task in every variant. The bench does not return late
   receipts inside the window, so no receipt is lost here.

Limitations: three runs per variant, one host, and the proxy's 35 s hold on the held
replica, which keeps late receipts out of the bench. The retry path is not exercised,
because no task fails in these runs, so the immediate-retry effect the review raised
is not measured here.

Decision: the wakeup is kept, since it removes the tick delay without changing
billing or the default drain. Whether the default drain should be 1 s is still a
billing decision and remains with the operator, as recorded above.

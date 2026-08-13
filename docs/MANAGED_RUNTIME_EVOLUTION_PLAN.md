# Managed Runtime 演進計畫：圖靈完備與科學運算

## 0. 先講結論

目前的 `managed-function-runtime` 是刻意設計成 deterministic、bounded、可計量的 JSON DSL。它適合做可驗證的函式計費與 proof settlement，但不能誠實宣稱是「部署時圖靈完備」或「完整科學運算環境」：目前的值域只有有限的整數、布林、字串、list、dict、null，且每次執行有固定的 operation、loop、call-depth、value、output 與 materialization 上限。

本計畫不把這兩個互相衝突的目標硬塞進同一個 v0 執行器，而是採雙 runtime 契約：

| Runtime | 目的 | 宣稱 | 結算方式 |
|---|---|---|---|
| `managed-function-v0` | 小型 deterministic 函式、可 proof／meter 的結算 | 有限 DSL；不宣稱圖靈完備 | 保留現有 proof settlement |
| `general-compute-v1alpha1` | 一般程式與科學工作負載的 pre-release 契約 | 採 pinned CPython 的一般程式語義；單次執行仍受資源配額限制 | 僅允許 allowlisted beta；usage 先視為 claim，不冒充 v0 proof |
| `general-compute-v1` | 通過 M0–M5 後的穩定契約 | 只宣稱 support matrix 與 gates 已證明的 CPU／GPU 能力 | 依 Nodepool 驗證出的 evidence level 結算 |

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

先新增 `general-compute-v1alpha1`，不改變 `managed-function-v0` 的語義。只有完成 M0–M5 的 schema、tensor ABI、sandbox、verifiability、migration 與 release gates 才能升為 `general-compute-v1`。目前程式碼中過早使用的 `general-compute-v1` 常數必須在 M0 改為 alpha id，避免未凍結契約被誤認為穩定 API。

`managed-function-v0` 的 runtime id、cost-model id 與 RISC Zero guest image 是同一個 proof binding；任何語義或計量改動都必須產生新 runtime／cost-model id、guest image、fixture、attestation 與 rollout，不能在 v0 原地擴充。若未來需要 richer deterministic DSL，另開 `managed-function-v1`，不要與完整 Python scientific backend 混為一談。

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

- `executor-rs/crates/managed-function-runtime`：維持 v0 DSL、canonical renderer、metering 與 proof guest 共用語義。
- `executor-rs/crates/general-compute-runtime`：新增 request/result schema、supervisor、quota、cancellation、artifact、capability 與 backend adapter；不得依賴 Hivemind database。
- `hivemind-rs/crates/worker-executor`：依 runtime version 分派 v0 或 v1；保留 detached supervisor 的 kill/reap cleanup guard。
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
- evidence envelope（`unverified`、`replicated`、`tee_attested`、`zk_proved`）；此欄由 Nodepool 驗證後衍生，Worker 不得自行決定 verified level。

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
- v0 繼續走目前 proof settlement。v1 alpha 的 resource telemetry一律存為 `worker_usage_claim`，不得單獨驅動 variable settlement；先採 Nodepool-owned fixed reservation/tariff，或在 replicated/TEE/zk evidence驗證後才升級可信度。Replicated output只能提高結果可信度，不能證明 CPU/GPU usage。
- evidence level由 Nodepool在驗證後寫入；Worker只能附 evidence bytes，不能宣告自己是 `tee_attested`或`zk_proved`。
- 每個版本同時鎖定 runtime version、cost model、proof/attestation protocol、guest image、trust pin 與 golden fixtures；任一項變更都產生新版本。

## 6. 里程碑與 Definition of Done

| Milestone | 交付物 | 必須通過的 gate |
|---|---|---|
| M0a 凍結 v0 | semantics/cost manifest、既知 Unicode/overflow/partial-receipt限制、proof vectors | v0 image/claim/receipt fixtures不漂移；公開文件不誇大 |
| M0b v1alpha 契約 | IDs、request/result/evidence validator、artifact/tensor schema、typed capability、threat model | property/fuzz/replay/unknown-field/shape-overflow tests；runtime id仍為 alpha |
| M1 圖靈核心與 sandbox | pinned CPython harness、trusted backend registry、rootless supervisor、Minsky/recursion/heap/cancel fixtures | differential + fuzz；timeout/cancel/drop/leader-exit全部 kill/reap；hostile escape tests |
| M2 CPU 科學 | tensor ABI、dtype/complex、broadcast/reduce、BLAS/LAPACK、FFT、ODE、RNG、Monte Carlo、sparse | NumPy/SciPy/reference golden；誤差與 failure semantics gate |
| M3 Worker/Nodepool/CAS 接線 | typed runtime routing、attempt binding、CAS/chunk transfer、quota/telemetry、evidence/idempotency | forged hardware/image/output/usage/evidence/replay皆拒絕；重派不重複結算；大檔 resume；多節點 E2E |
| M4 GPU beta | CUDA/ROCm image、driver matrix、device artifact、CPU fallback | capability mismatch 不誤派；GPU/CPU 結果與成本標記正確 |
| M5 可用性發布 | canary/migration、文件、SDK 範例、benchmark dashboard、signed image/SBOM、support matrix、回滾 | reproducibility、security、performance、compatibility 全簽核後才把 id升為 `general-compute-v1` |

M3 trusted capability registry gate 已落地：Nodepool operator config 是 worker general-compute capability snapshot 的唯一來源；registration 以 owner binding 寫入 Postgres，untrusted heartbeat 不得覆蓋 snapshot，owner-authorized registration 可撤銷 snapshot，scheduler 對 `general-compute-v1alpha1` 僅依 persisted snapshot 與 request matching 做 admission。Attempt-bound request/result compatibility 也已完成；inline artifact materialization 已完成第一個獨立 checkpoint。下一個 gate 是 Worker typed backend execution，再接 CAS/chunk resume 與 Nodepool typed result settlement。

每個 milestone 必須提交：測試命令與結果、fixture/hash 清單、benchmark 原始資料、已知限制、rollback 方法與明確 owner。沒有這五項，只能算 prototype，不能標示為 production-ready。

## 7. 效能、可重現性與營運門檻

- 先在固定 CI runner 建立 CPU、記憶體、artifact、cold-start、warm-start、BLAS、FFT、ODE 與 GPU baseline；後續 PR 若 p95 退化超過 20% 必須阻擋或附上核准的理由。
- warm sandbox 的 supervisor overhead 目標不超過同等直接 backend 執行時間的 15%；cold-start、image pull、CAS throughput 與 GPU initialization 分開報告，不能混成一個平均數。
- `strict_reproducible_cpu` 固定 architecture/features、rounding、單 thread、BLAS/FFT與 reduction policy；`reproducible_same_profile`只承諾同 image/hardware profile；GPU/parallel CPU預設 `best_effort`並用數值 acceptance rules，不宣稱跨硬體 bitwise一致。
- 觀測至少包括 queue latency、startup、CPU/wall ratio、peak RSS、I/O、GPU time/VRAM、cancel/timeout、artifact retry、backend mismatch、reproducibility mismatch 與 unverified claim count。
- 發布前必須做長跑 soak、重派與節點故障、CAS 中斷續傳、同一任務重播、惡意輸入、超大 shape、NaN/Inf、fork/thread bomb 與 container escape 測試。
- benchmark保存 raw data與 p50/p95：sandbox cold/warm start、cancel latency、CAS upload/download/checksum/resume/dedup、elementwise/reduction、DGEMM多尺寸、FFT、SpMV、ODE、Monte Carlo、CPU thread scaling、GPU init/transfer/kernel/fallback；v0 native與proof path是不可退化 baseline。

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

`general-compute-runtime` 已有 contracts、capability validation、framed JSON、bounded supervisor與 output capture；這只是 M0/M1 scaffold，尚未接 Worker，也沒有 sandbox、CAS、tensor ABI、scientific image或可信 billing。Master、Nodepool、Worker目前仍只接受 v0 managed path。

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

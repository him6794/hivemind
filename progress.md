# Hivemind 驗證進度

## ZK 函式計費證明（2026-08-07）

- overall: `running`
- current step: 階段 2，將已驗證的 RISC Zero receipt 封裝為 Worker proof envelope
- completed this round:
  - 確認現有系統沒有真正 ZKP，receipt 是未驗證 Worker claim
  - 確認 trust model 要求 Nodepool 獨立驗證 Worker 計費聲明
  - 決定採成熟 zkVM 證明完整 managed runtime 執行
  - 建立分階段 rollout 與成功標準
  - RED 測試確認 proof claim API 缺失後，完成最小 GREEN 實作
  - 建立 protocol/runtime/cost-model 版本、task binding、SHA-256 source/input/output commitments 與 budget binding
  - 移除會新增未維護依賴警告的 postcard/bincode，改用既有 serde_json journal
  - 固定 RISC Zero 3.0.6 stable；production verifier 強制 `disable-dev-mode`
  - 將 canonical output renderer 從 Worker 下沉到 managed runtime，避免 guest/host commitment 分歧
- next action: 以 RED 測試定義可序列化 proof envelope，接入 Worker 產生路徑；Nodepool verifier 尚不接結算
- blockers: RISC Zero 3.0.6 transitive lockfile 有 2 個 audit advisories，需在發布前隔離或建立可稽核 ignore policy；單次 proving 約 9.5 分鐘，不可直接 enforce
- remote actions: none（不 push、不建立 PR）

### 本輪測試結果

| 測試 | 結果 |
|---|---|
| proof-contract RED | 如預期因 API 不存在而失敗 |
| `cargo test -p hivemind-managed-proof --lib` | 3 passed |
| proof crate clippy `-D warnings` | passed |
| GNU workspace all-target/all-feature tests | 246 passed, 0 failed |
| `cargo audit` | 0 vulnerabilities；2 個既有 allowed warnings |
| MSVC workspace test | 既有 MinGW `libtailscale.a` linker 不相容；改用 GNU target 驗證 |
| canonical renderer RED | 如預期因 runtime API 不存在而失敗 |
| managed runtime | 16 passed；clippy/fmt passed |
| Worker executor GNU | 52 passed；clippy/fmt passed |
| zkVM host MSVC | 首次失敗：`risc0-circuit-keccak-sys` 傳 `/std:c++17`，但來源使用 C++20 designated initializers；不原樣重試，改走 Linux Docker |
| zkVM host GNU（首次） | 5 秒工具時限內仍在編譯；無測試結果，下一次使用較長可觀測時限 |
| zkVM host GNU（第二次） | 64 秒工具時限內仍無錯誤輸出；cold build 尚未完成，保留增量產物 |
| zkVM host GNU（完成診斷） | 失敗：methods build script 仍由 MSVC host 編譯，`risc0-zkvm-platform` 缺少 `sys_alloc_words`；確認必須使用 Linux host |
| Linux zkVM host（首次） | builder digest 驗證成功；host cold build 因 Rust 1.88 低於 workspace MSRV 1.90 而停止，測試映像改固定 1.90 |
| Linux zkVM host（第二次） | Rust host 依賴已成功編譯；guest build 因缺少 guest `Cargo.lock` 與 RISC Zero Rust toolchain 停止，進入工具鏈修正 |
| Linux test image toolchain | rzup 0.5.2 與 RISC Zero Rust 1.97.0 已完成建置層；Docker Desktop 在最後匯出時 RPC EOF，待從 BuildKit cache 重新匯出 |
| Docker recovery | C: 僅剩 30 MB 導致 Docker VHD ext4 I/O error；清除 3.56 GB 可重建 npm cache 後受控重啟，8 個既有 Hivemind 容器全數恢復，PostgreSQL/Redis/Nodepool healthy |
| Linux zkVM host（第三次） | D: toolchain/target 路徑工作正常，host 編譯完成大半；guest builder 因 outer image 缺少 Docker buildx plugin 停止，下一次掛入 plugin 後沿用快取 |
| Linux zkVM host（第四次） | buildx 0.20.1 已正確載入，但 guest BuildKit 仍擴張 C: Docker VHD 至零空間並讓 daemon EOF；停止 Docker prover 路徑，避免再次中止測試 stack |
| WSL native host（首次） | RISC Zero Rust 1.97 可直接執行；Docker 建立的 target cache 權限不相容，切換獨立 WSL target cache |
| WSL native host（第二次） | 已成功編譯大量依賴；Docker root 建立的 Cargo registry 個別檔案不可讀，下一步修正 cache 權限後增量續跑 |
| WSL native host（第三次） | root 已越過 cache 權限並編譯至 RISC Zero circuits；WSL artifact endpoint 回 400，改用相同絕對路徑重用 Docker 已完成 artifacts，避免任何外網下載 |
| WSL native host（第四次） | 標準 Rust 1.90 host 成功完成全部 RISC Zero circuits（S3 artifact 不再阻塞）；目前只剩 local guest methods build 的隱藏 exit 101，進入 debug 診斷 |
| zkVM guest GREEN | `risc0-zkvm/std` 修正後 guest ELF 已成功建置；host compile RED 揭露 Journal/raw bytes API 型別錯誤，已改讀 `journal.bytes` |
| real receipt RED | `receipt_verifies_guest_image_and_commits_native_claim` 如預期因 `prove_guest_execution` 不存在而 E0432；已加入最小 prover API，待 GREEN 驗證 |
| real receipt/tamper GREEN | 2 tests passed；真實 receipt 驗證固定 image ID，錯誤 image ID 與篡改 journal 均被拒絕；proving 579.77 秒 |
| zkVM quality gates | fmt passed（僅既有 stable/nightly-option warnings）；clippy `-D warnings` passed with `RISC0_SKIP_BUILD=1`；audit 發現 2 個 RISC Zero transitive vulnerabilities，正在判定可達性/升級路徑 |

## 前一輪平台驗證

## 目前狀態

- overall: `complete`
- owner: Codex
- blockers: none
- remote actions: none（未 push、未建立 PR）

## 最終結果

- Managed runtime：15 passed。
- GNU backend workspace：243 passed，0 failed。
- Site / Master UI / Worker UI：13 / 14 / 10 tests passed，三個 production builds passed。
- PowerShell release contracts：8/8 scripts passed。
- Release frontend previews：3/3 surfaces passed，且 cleanup ports 驗證通過。
- Docker release stack：5/5 health surfaces passed。
- Playwright：2/2 release flows passed。
- Rust fmt / check / clippy：passed。

## 已修復

- Managed task cancellation。
- Windows excluded-port 與 smoke volume isolation。
- 動態 UI API base / CORS。
- Billing-aware E2E task budgets。
- Windows preview child-process cleanup。

## Cleanup

Docker validation resources 已移除；native PostgreSQL 已停止。安全政策阻擋遞迴刪除，因此尚留 inactive validation-only 目錄 `D:\hivemind-validation-postgres-20260807`。

詳細證據見 `docs/platform-validation-state.md`。

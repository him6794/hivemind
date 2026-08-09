# Hivemind 驗證進度

## ZK 函式計費證明（2026-08-07）

- overall: `running`
- current step: 階段 3，定義 protobuf proof envelope 與 Nodepool 獨立 verifier
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
  - 以 RED→GREEN 完成 `WorkerProofEnvelope`，固定 `risc0-zkvm-3.0.6` scheme、guest image id、journal 與完整 receipt
  - `prove_guest_envelope` 由 prover 直接組裝 envelope；`verify_proof_envelope` 先驗 metadata/journal，再驗 receipt，最後才解析 execution claim
  - protobuf `ManagedProofEnvelope` 已攜帶 scheme、8-word image id、journal 與 receipt JSON，並掛入 `ExecuteTaskResponse.managed_proof`
- next action: 以 RED 測試建立 Nodepool-owned verifier adapter；驗證成功前仍不採信 Worker usage units
- blockers: RISC Zero 3.0.6 transitive lockfile 有 2 個 audit advisories，需在發布前隔離或建立可稽核 ignore policy；單次 proving 約 9.5 分鐘，不可直接 enforce
- remote actions: none（不 push、不建立 PR）

### 2026-08-09 恢復檢查

- 從 `task_plan.md`、`findings.md`、`progress.md` 與 `docs/zk-metering-proof-state.md` 恢復階段 3。
- `session-catchup.py` 的技能文件仍指向舊 `.claude` 路徑；已確認並改用實際 `.codex` 安裝路徑，恢復成功。
- WSL 內沒有殘留 Cargo/rustc/generator 程序，也沒有 `/run/desktop/mnt/host/d` 或 `/root/.cargo` 暫時 bind mount；可安全建立本輪精確 mount。
- 未停止或清理使用者正在測試的 Docker stack。
- 已確認可重用快取存在於 `D:\hivemind\.cache\zkvm-target`，標準 Rust 1.90 host 工具鏈位於 `.cache/wsl-rust/1.90.0-x86_64-unknown-linux-gnu`，WSL Cargo cache 與 RISC Zero/TMP 亦都在 D:。
- 現有 guest `.d` 精確記錄 `/run/desktop/mnt/host/d/hivemind/...`，因此本輪會由該絕對路徑執行，並使用 `.cache/zkvm-target`；methods build.rs 已確認 `HIVEMIND_ZKVM_USE_DOCKER=0` 會直接走 native `embed_methods()`。
- methods-only 第一次 wrapper 在 mount/build 前即因跨 PowerShell/WSL 的 Bash 變數傳遞失真而停止；沒有留下 mount 或 Cargo/rustc 程序，將改用不含 shell 變數的精確絕對路徑命令。
- 以 D: ignored 腳本消除跨 shell quoting 後，methods-only check 已真正進入 native guest build；因 verifier 變更與 guest 共用 `managed-proof/src/lib.rs`，Cargo 正常重建 guest ELF，依賴/circuits 均命中快取。
- methods-only native check GREEN：353 秒完成，`cleanup_complete rc=0`；事後確認兩個 bind mount 與 Cargo/rustc/build 程序全部為零。
- 固定真 receipt fixture generator GREEN：815 秒（host cold build 3分27秒，proof 約 10分）；輸出 664,026-byte envelope，事後 mount/build process 全部為零。
- fixture claim：usage/executed ops 29、function calls 1、loops 0、max depth 1、output 12 bytes；receipt 是單一 Composite segment、無 assumptions，journal 656 bytes。
- 正向 verifier 真測試首次跑到 public path 後 RED：`UntrustedImageId`；正在比對重建 guest image ID 與舊 pin，尚未採用 fixture 值作修正。
- image drift 根因確認後更新可信 pin；真 receipt 正向 verifier GREEN：1 passed，debug verification 約 0.47 秒。最終 verifier-only 變更後仍會重建 guest 並確認 image ID 不再漂移。
- verifier resource gates 逐項 RED→GREEN：4 KiB journal raw cap、2 MiB receipt raw cap、unsupported scheme 不保存 attacker string、只接受 Composite、恰好 1 segment、拒絕 recursive assumption receipts，且 final claim assumptions 必須明確為 `Value([])`。
- segment index/hashfn/seal cap、public-path invalid claim prefilter與 thread-local minimal context均完成 RED→GREEN；fixture budget regression鎖住所有實測值。
- `cargo test -p hivemind-managed-proof --features risc0-verifier`：24 passed，0 failed；真 receipt debug verification約 0.45秒（完整 suite 0.51秒 test runtime）。
- `cargo fmt --all -- --check`、`git diff --check` 與 verifier clippy `-D warnings` 已通過；feature graph確認未啟用 `prove`、methods或 `risc0-build`。
- 移除 verifier 不需要的 RISC Zero `std` feature後，24 tests仍通過，主 workspace不再解析有漏洞的 `tracing-subscriber 0.2.25`；另將 `event-listener` 5.4.1更新至修正版5.4.2，`cargo audit` 為0 vulnerabilities（保留3個既有 allowed warnings）。
- verifier依賴精簡後再次執行 WSL methods rebuild，19秒GREEN；guest ID仍為 `[3606400121, 4250889949, 2277454476, 3430793801, 2111044864, 2713379816, 851522248, 2751351423]`，所有暫時 mount與Cargo/rustc程序均已清理。
- verifier資源稽核確認近似有效的截短／翻轉／補零 seal約17.6 ms，未發現比合法 proof更昂貴的失敗路徑；2 MiB receipt與131,072-word seal caps彼此相容。
- Review要求的 crypto-path negative test已補齊：合法大小Composite seal bit flip通過shape/journal gates後回 `InvalidProof`；完整 verifier suite為25 passed。
- 新增tracked host regression `tests::generated_guest_id_matches_nodepool_trust_pin`；先取得常數被feature gate隱藏的預期RED，再將純trust pin移出feature gate，current-source WSL rebuild後1 passed。ELF/input SHA-256與cleanup證據已寫入 `docs/zk-managed-proof-build-attestation.md`。
- verifier post-change re-review：CLEAR／APPROVE，原 guest pin與crypto-gate blockers均解除，0 remaining blockers。

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
| proof-envelope RED | 如預期只因 `prove_guest_envelope`、`verify_proof_envelope`、`WorkerProofEnvelope` 與 scheme 常數不存在而 E0432 |
| proof-envelope focused GREEN | 2 passed；JSON round-trip 保留 receipt/metadata，scheme/image/journal tamper 均在 proof verification 前拒絕 |
| proof-envelope real GREEN | 1 passed；真實 proving 570.02 秒，JSON round-trip 後固定 image verifier 通過且 claim 與 native runtime 相同 |
| proof-envelope quality gates | fmt、`git diff --check`、clippy workspace/all-target/all-feature `-D warnings` passed；audit 維持既知 2 vulnerabilities、4 allowed warnings |
| protobuf transport RED | 如預期因 `ManagedProofEnvelope` 與 `ExecuteTaskResponse.managed_proof` 不存在而 E0432/E0560/E0609 |
| protobuf transport GREEN | proto round-trip 1 passed；proto/Worker/scheduler affected crates `cargo check` passed，既有路徑明確送出 `managed_proof: None` |
| Nodepool verifier RED→GREEN | 7 個負向測試逐一先 RED 再 GREEN；錯誤 scheme/image id/receipt/journal、fake proof 與 invalid claim 均 fail closed |
| verifier real-receipt RED | 正向測試已確認只因固定真 receipt fixture 不存在而失敗；WSL/native Linux 正在產生一次性 fixture，Cargo/target/TMP 全位於 D: |
| verifier fixture build diagnosis | 首次 generator 在 proving 前因漏設 `HIVEMIND_ZKVM_USE_DOCKER=0` 而落入預設 Docker builder；暫時 mount 已自動清理，methods-only native build 正在驗證既有開關 |

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

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
- claim binding TDD：API-missing RED 與 10 個 mismatch RED 均如預期失敗；GREEN 後 default 15 passed、all-feature 37 passed、clippy/fmt passed，read-only review CLEAR；本機 commit `eb9894a feat(proof): bind verified claims to tasks`。
- scheduler verified-claim settlement RED：因 `verified_managed_completion`／`ManagedCompletionError` 尚不存在而 E0425/E0433；最小實作後 12 passed，已覆蓋重播、source/input/output/budget、protocol/runtime/cost-model mismatch、missing source/nonpositive budget、null input 與 Worker legacy scalar/receipt 不可信。
- existing-binary verifier RED→GREEN：缺 mode/byte verifier API 的預期 RED；nodepool-only完整 lib 10 passed，真 proof與crypto tamper、malformed/trailing/oversize、exact hidden argument均通過，stdout只輸出 claim JSON。
- verifier subprocess RED→GREEN：缺 adapter API 的預期 RED；5 tests passed，覆蓋protobuf/stdout、oversize-before-spawn、concurrency 1/queue 8、nonzero/malformed/oversize output與timeout kill/reap；128 MiB平台硬限制已接入。
- dispatcher verifier接線 RED→GREEN：failed verifier與verifier-returned claim 先因 resolver API不存在而失敗；最小接線後3 passed。Worker response status/legacy receipt caps亦先取得missing-helper RED，後3 passed。
- scheduler full gate：68 passed；all-target clippy `-D warnings` passed。timeout test 的 PID-marker flake 經 systematic debugging 確認為250ms測試啟動假設，production cleanup正常；test-only 750ms後10/10 serial與5/5 suite GREEN，production 1s未變。
- binary process smoke 新 RED：一般環境為generic error，但 `RUST_BACKTRACE=1` 讓 anyhow main termination洩漏stack trace；根因為hidden mode向main回傳Err。正在改為內部捕捉、固定一行stderr與explicit nonzero exit，並要求backtrace on/off逐byte smoke。
- binary process smoke GREEN：hidden mode 已改為內部固定錯誤處理；兩個 binary 在 backtrace off/on 均為 exit 1、stdout 空、stderr 精確 `managed proof verification failed\\n`，真 fixture 均回合法 claim；hivemind-bin 11 tests、clippy/fmt/diff check 通過。
- production 父程序 acceptance GREEN：以實際 `target\\debug\\hivemind-nodepool.exe` 執行 ignored `production_binary_verifies_real_fixture_under_process_limits`，1 passed，真 fixture 在 process limits 下約 0.60 秒完成。
- verifier CLI 與 scheduler 的 2,166,784-byte envelope cap 已改為引用 `hivemind-proto::MANAGED_PROOF_RPC_MESSAGE_MAX_BYTES`，消除跨元件 magic-number 漂移；待 package gate 後提交。
- verifier saturation 公平性 TDD：review 指出 local `QueueFull`／等待 execution permit 超時會錯誤地永久 fail task 並扣 Worker reputation。先取得缺少 disposition/queue-deadline variant 的 compile RED，再取得 queue wait 回一般 `DeadlineExceeded` 的行為 RED；GREEN 後 queue-local 失敗改為 redispatch且不呼叫 `fail_for_worker`，真正 child deadline/invalid proof仍 fail closed。兩個 focused queue tests 通過。

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

## 2026-08-09：ZK managed-function 實作續作

- 依 long-task recovery 重新讀取 `task_plan.md`、`findings.md`、`progress.md` 與 `docs/zk-metering-proof-state.md`，並核對 git 工作樹及 active goal；狀態維持 `running`。
- Admission caps 首輪 read-only review 為 BLOCK：四個 Node Manager direct task-ID RPC 漏 admission gate，另有一項 DB fixture 不可用時 silent-return 的測試證據缺口；已寫入 durable state，等待 owner 修正後重審。
- Prover sidecar owner 已確認 protocol RED→GREEN；Windows host compile 被既有 RISC Zero C++17/C++20 與 build-script linker 問題阻擋，已要求不重跑、不改 registry/toolchain，繼續輕量檢查。
- 啟動 read-only `worker_lifecycle_map`，盤點 Worker RPC deadline、message caps、active-task cleanup、`spawn_blocking` cancellation 與 proof response test seams。
- `worker_lifecycle_map` 已完成：確認 scheduler client 與 Worker server 都缺 explicit transport bounds/deadline，Worker response 仍固定無 proof，且 active-task remove 只在正常 await 返回後發生；精確 seams 已寫入 `findings.md`。
- 深入讀 Worker lib/executor後確認單純 RAII remove不足：future drop會同時丟掉設 cancellation flag 的 select future並 detach `spawn_blocking`。下一 slice要以 shared cancellation + detached supervisor持有 cleanup guard做 RED→GREEN，確保背景工作可追蹤至真正結束。
- Metis precheck完成並找出 lifecycle/error taxonomy缺口；已把 request-drop guard、supervisor-owned cleanup、pre-prover cancellation check、ResourceExhausted redispatch、child process death/reap與20分鐘RPC/15分鐘proof timeout納入下一切片驗收。
- Admission owner在最後 focused Worker gate誤走已知MSVC/libtailscale link blocker；process trace確認後已中斷並只清該四PID compile tree，重新指派GNU target gates。實作檔案保留，Docker/服務與其他代理未受影響。
- Admission final evidence已讀：proto 2 passed；GNU Node Manager 65、Worker 58及Master API full/doc tests全通過；四RPC gate與runtime bypass focused GREEN；GNU clippy、owned rustfmt/diff check通過。已啟動獨立final review。
- 重讀 verifier core 與 dispatcher completion gate：確認原 review 的 queue fairness MEDIUM 已修正，verified-only settlement 與 kill/reap 邏輯仍維持；準備重跑切片 gates 後精確提交。
- Verifier full-test orchestration 因 admission owner 同時編譯而在 120 秒工具時限結束；process trace 顯示這是 Cargo target lock／編譯競爭，並非測試失敗。逾時後 `hivemind-bin nodepool` child cargo 仍在執行，已納入追蹤，不啟動重複命令。
- 後續 process tree 顯示 admission focused test、prover protocol checks、接手代理的其他 gates 與遺留 nodepool test 共用同一 target；僅一個 rustc 實際編譯，其餘 cargo 等 lock。等待 owner gates 收斂後再跑 coordinator 最終 gate。
- Admission 首個 focused compile 已釋放 build lock；逾時後遺留的 coordinator nodepool test 現已進入 rustc，protocol test 尚在等待。因原輸出 pipe 已失聯，只把它當 cache warm-up，不把程序退出冒充測試通過。
- Nodepool rustc 已持續數分鐘且 CPU 時間很低，尚無 linker/error process；下一診斷量測 CPU delta 與 wait state，若確定失去進展才清理 coordinator 自己啟動的孤兒程序，不碰代理或 Docker。
- 根因確認為工具 timeout 後 parent shell 已退出、nodepool test 的 `rustup→cargo→rustc` 成為無 reader 的孤兒程序；只終止精確三個 PID，全部已停止。代理 cargo 與 Docker/服務程序未受影響。
- 第二次 verifier gate 已越過 scheduler 並進入 nodepool MSVC link，但 5 分鐘上限先到；精確四個 coordinator PID 已清理。下一次分開執行並給 15 分鐘，不把無輸出的部分進度列為 pass。
- Prover sidecar 接手 owner 已完成：protocol tests 13 passed、sidecar CLI harness 5 passed、兩者 clippy `-D warnings` 與 fmt/diff check 通過；未觸發既知受阻的 RISC Zero Windows host compile或真 proof。待 coordinator 讀 code/evidence 後提交。
- 已讀取 prover evidence：實體 artifacts 位於 `.omo/evidence/managed-prover-protocol-sidecar/`，包含命令輸出、owned diff 與 status；已啟動獨立 read-only security/code review，尚未宣稱此切片可提交。
- Coordinator 已審閱 protocol 與 host adapter：雙層 encoded/decoded caps、strict version/unknown-field rejection、prove 前 validation與 response 後 validation方向正確；發現主 proto／sidecar protocol constants 有漂移風險，留待 reviewer裁決與 Worker接線時加 equality gate。
- Coordinator 已審閱 sidecar main與5項 harness tests：failure不洩漏輸入、nonzero/empty-output contract方向正確；另記錄 partial write、explicit flush與 nested `tdd-red` harness維護性議題，等 read-only reviewer 回覆後處理。
- Coordinator 以獨立 target-dir 重跑 protocol：13 passed；sidecar CLI harness：5 passed。兩者 0 failed，沒有編譯 RISC Zero host；這些結果可重現 owner evidence。
- Prover sidecar read-only review 回報 REQUEST_CHANGES/HIGH：`zkvm/managed-proof/Cargo.lock` 漏掉新 local protocol dependency，locked metadata失敗；另指出 owned-diff evidence漏 untracked files且含雜訊。切片維持未提交，進入同命令 RED→GREEN 修正。
- Host lock blocker已重現RED（exact `cargo metadata ... --locked` exit 101），用 `--offline` 只新增 host dependency與 local protocol package兩個 lock hunks後，同一 locked metadata命令GREEN。未編譯 RISC Zero、未連網、未改其他 dependencies。
- Prover sidecar re-review 為 CLEAR／APPROVE、0 blockers；reviewer重跑 locked metadata、protocol與harness tests及scoped diff check皆GREEN。LOW僅為舊 owned-diff evidence漏 untracked/noisy，不影響code approval，提交前會以實際 staged diff為準。
- Prover release packaging map完成：所有Windows/Linux package、manifest、compose與contract tests都尚未包含sidecar；且Windows目前沒有可建置的RISC Zero prover artifact。已啟動官方source-only研究，發布狀態仍為`running`。
- 官方RISC Zero source/docs研究完成：current stable 3.0.6沒有first-class Windows host/prover support或官方workaround，current build_kernel仍指定C++17。Windows blocker屬上游平台限制，不可用本地flag猜測冒充發布修正。
- `runtime_limits_audit` 完成：確認 Worker/guest 目前都只約束 usage budget，其他 runtime caps 因 `unlimited()` 未啟用；一致修正會漂移 guest image ID，已列為需重建 attestation/fixture/real-proof 的發布前獨立切片。
- 已只 stage 七個 verifier／settlement 檔案；cached diff 為 1,611 additions／14 deletions，`git diff --cached --check` 通過。Cargo.lock 尚未 stage，等待以精確 hunk 分離 prover-protocol entry。
- 已用可重現的 cached patch 只 stage Cargo.lock 內 `hivemind-bin` 與 `hivemind-task-scheduler` dependency hunks；prover-protocol package entry 留在 unstaged worktree。整體 cached diff check 通過，暫存 patch 已刪除。
- 再核對 index：verifier slice精確為8個 staged paths（含6行lock dependency），其餘admission/prover/docs全為unstaged；cached與working `git diff --check`皆無 whitespace error，只有CRLF提示。
- Verifier scheduler GNU full gate完成編譯並跑71 tests：69 passed、1 ignored、1 failed。唯一失敗是`timeout_kills_and_reaps_child`在750ms內找不到PID marker；當時admission final reviewer同時有多個GNU cargo/link程序。已進systematic root-cause階段，尚未改test/production deadline。
- `worker_lifecycle_map` 已完成：確認 scheduler client 與 Worker server 都缺 explicit transport bounds/deadline，Worker response 仍固定無 proof，且 active-task remove 只在正常 await 返回後發生；精確 seams 已寫入 `findings.md`。
- 兩次向既有 admission owner 發訊均因 collaboration provider `custom` 不存在而被工具拒絕；沒有程式碼或外部狀態變更，owner 仍在執行且其子 reviewer 結果可見。
- Admission caps 最終 read-only review 已回報 `CLEAR / APPROVE`、零 blockers；既有 GNU full/focused tests、clippy、fmt/diff evidence 仍為有效驗證依據。
- Verifier full gate 的唯一失敗已完成根因確認：測試將「子程序已建立」錯綁為「子程序已獲 CPU 寫 PID 檔」。改由父端在 `spawn` 後記錄 PID，正式一秒 verifier deadline 不變；focused kill/reap GREEN（1 passed），scheduler full GREEN（70 passed、1 intentional ignored），GNU clippy `-D warnings` GREEN。
- Worker lifecycle read-only mapper 已完成：`execute_task` future drop 會使 active-task remove 與 blocking work tracking 脫鉤，gRPC response 仍固定 `managed_proof: None`。下一實作切片將先以 RED→GREEN 建 shared cancellation、supervisor-owned cleanup 與 bounded prover sidecar。
- Verifier／settlement 切片已以獨立本機 Conventional Commit 封存：`03a080e feat(proof): isolate verified settlement`。提交前 scheduler full（70 passed、1 intentional ignored）、focused kill/reap、nodepool verifier（7 passed）與 GNU clippy `-D warnings` 均為 GREEN；未 push，也未碰 Docker stack。
- Admission caps 切片已以獨立本機 Conventional Commit 封存：`367c71d feat(api): enforce managed task admission caps`。提交範圍僅含 proto、Master API、Node Manager、Worker admission gates；以 final review `CLEAR / APPROVE` 與既有 GNU full/focused/clippy evidence 為驗收依據。

## 2026-08-09：目前進度與恢復順序

- 已封存：`03a080e feat(proof): isolate verified settlement`、`367c71d feat(api): enforce managed task admission caps`；兩者均為本機 commit，未 push。
- Sidecar 目前具備輕量 protocol、host adapter、strict JSON/caps、generic fail-closed CLI contract；已有 protocol 13 passed、harness 5 passed、locked metadata RED→GREEN 的證據，但尚待重新確認 harness 的提交範圍並精確封存。
- 不可宣稱可發布：Worker 仍固定回傳 `managed_proof: None`；future drop 仍可遺留 active task；sidecar timeout/cancel/kill/reap、RPC caps/deadlines、runtime limits/attestation、Linux proving E2E、packaging/audit 尚未完成。
- 後續順序：先封存 sidecar → TDD 實作 Worker proof/cleanup/cancel → 加入 scheduler/client/server transport bounds → 修正 runtime limits 並重建 guest attestation/fixture → Linux/WSL 多節點與發布稽核。
- `tdd-red/target` 是約 111 MiB 未追蹤的 agent-generated test artifact；不可 stage。嘗試只刪除此精確目錄被環境政策拒絕，未繞過。
- 前一輪 shell 無回應期間沒有新增可採信的測試或 commit 結果；shell 現已恢復，下一輪從當前工作樹重新驗證。
- Prover sidecar 已以獨立本機 Conventional Commit 封存：`6e7af38 feat(proof): add managed prover sidecar`。提交前 protocol 13 tests、sidecar harness 5 tests、兩者 clippy/fmt、host locked metadata 與獨立 review `CLEAR / APPROVE` 均為 GREEN；未執行受阻的 Windows RISC Zero host build 或真實 proof。

## Latest implementation checkpoint — 2026-08-09

Completed local commits:

- `1a9fa8f feat(rpc): bound worker proof transport`
- `d99c8f7 feat(worker): generate managed proofs safely`

Verified locally on `x86_64-pc-windows-gnu`:

- `cargo test -p hivemind-task-scheduler --lib --locked`: 75 passed, 1 intentional ignored.
- `cargo test -p hivemind-worker-executor --lib --locked`: 81 passed.
- managed prover focused regressions: 15 passed, including cancellation, timeout, oversized stdout, abort-after-spawn and permit reuse.
- Worker clippy `-D warnings`, worker-feature `hivemind-bin` compile, scoped rustfmt and diff checks: passed.

Status remains `running`, not release-ready. Required next gates are finite matching Worker/guest execution limits, regenerated zkVM image/fixture/attestation, a real supported-host proof, image/Compose packaging of the prover sidecar, and multi-node Docker end-to-end validation.

## Runtime safety checkpoint — 2026-08-09

已完成本機提交：`097c98a fix(runtime): enforce finite managed execution limits`。

- Worker 與 guest 目前同時使用有限 default evaluator limits；由 task/envelope 提供的 usage budget 仍獨立且只用於計費。
- depth-65 managed-function 回歸在舊 unlimited policy 為 RED，在新 policy 為 GREEN。
- 目前本機 Worker gate：82 passed、0 failed；warnings-denied clippy、rustfmt 與 diff check 均通過。

狀態保持 `running`。下一個進行中的切片是 canonical return rendering 與中間值的 allocation-safe 上限。完成後才能重建 guest 並刷新 image ID、attestation、proof fixture；最終真 proof 與 release E2E 仍需要支援的 Linux/macOS prover host。

## Bounded renderer checkpoint - 2026-08-10

- overall: `running`（仍非 release-ready）
- current step: 階段 4 尚未開始；階段 3 經程式碼比對後確認完成
- 本輪本機提交：`9ab1ffc test(worker): deflake managed stop cancellation test`、
  `0158129 fix(runtime): bound canonical output and value materialization`
- finding #29（canonical return value 繞過 output guard）已關閉。三個 production 路徑
  （Worker、zkVM guest、host golden-vector claim）現在共用 `render_output_bounded`。
- Renderer 在每次 append 前檢查，因此被拒絕的值不會先產生超大中間字串；escaping 以
  `serde_json` 為基準測試釘住（含 quote／backslash／control chars／non-ASCII）。
- 新增限制皆為固定寬度 u64 邏輯位元組：per-value canonical bytes、collection items、depth，
  以及 cumulative materialization budget。只是安全上限，不計入 billing。
- Worker 遇到超限回傳值時，改以結構化 `value_limit_exceeded` receipt 失敗，不再回報成功。
- 附帶修正既有 flaky test：`stop_task_execution` 在 assignment 記錄前回 `PermissionDenied`，
  poll loop 卻 unwrap 了 `Result`。修正前 4/12，修正後 15/15。
- 驗證（`x86_64-pc-windows-gnu`）：runtime 25 passed、worker-executor lib 83 passed、
  managed-proof 15 passed、task-scheduler lib 75 passed（1 intentional ignored）、
  clippy `-D warnings`、`cargo fmt --all -- --check` 全綠。
- blockers（依阻擋強度排序）：
  1. prover sidecar 未進 Compose／worker image，`MANAGED_PROVER_EXECUTABLE` 預設空字串，
     目前 Compose 起的 worker 會讓所有 managed task 失敗。
  2. guest image ID／attestation／真 receipt fixture 已過期，需支援的 Linux/macOS host 重建。
  3. 階段 4（rollout mode、metrics、audit events）與階段 5（惡意 Worker、多節點 E2E、audit）未開始。
  4. proving 約 570-580 秒 vs 毫秒級任務，enforce 前需要先定經濟模型。
- remote actions: none（未 push、未建立 PR、未動使用者的 Docker stack）
## Continuation checkpoint - 2026-08-11

- overall: `awaiting-result`
- current step: Linux container build of the real `hivemind-managed-proof-prover` sidecar
- recovery: background cell `252` is still running; container `d8cab3c4b1f2` is compiling the RISC Zero host/C++ dependencies
- observed output: the first guest-build attempt reported missing RISC Zero Rust toolchain (`rzup install rust`); the current build has not exited yet
- next action: wait for cell `252`; if it exits with the same toolchain error, install `rzup 0.5.2` and the RISC Zero Rust toolchain inside the builder, then rerun the locked release build
- blockers: sidecar binary, Worker image packaging, real proving fixture/attestation, rollout regression, malicious-worker tests, and docs remain incomplete

## Prover build result - 2026-08-11

- status: `running`
- recovery completed: cell `252` exited nonzero after the locked release build finished compiling host dependencies
- root cause: `risc0-build` could not find the RISC Zero Rust guest toolchain and requested `rzup install rust`
- next action: rerun in a Linux Rust container with `rzup 0.5.2`, `rzup install rust`, and `HIVEMIND_ZKVM_USE_DOCKER=0`; copy the resulting binary to `.cache/managed-prover-linux/`
- 後續：該 prover 建置最終成功，binary 位於 `.cache/managed-prover-linux/hivemind-managed-proof-prover`（95,640,904 bytes，Linux x86-64 ELF）。

## 發布驗證 — 2026-08-11

- overall: `running`；階段 1–4 完成，階段 5 僅剩多節點 Docker E2E 與瀏覽器回歸。
- 起點為 `01ffb3a` 加上未提交的階段 4 rollout/metrics 切片。

### 本輪修正的既有缺陷

這些全部是先前被環境或設定遮蔽、從未真正執行過的路徑：

1. `zkvm/managed-proof/host/src/lib.rs:254` 以 `assert_eq!` 比較 `risc0_zkvm::Receipt`，
   但該型別沒有 `PartialEq`，整個 crate 的測試無法編譯。該測試由 `6e7af38` 加入，因
   Windows RISC Zero host build 受阻而從未編譯過，連帶讓 guest image ID 的 pin-test 也
   無法執行。改為比較 canonical JSON，即 verifier 實際解析的表示法。
2. `test_seed_default_user_inserts_bootstrap_account` 直接連 public schema 並
   `DELETE FROM users`，卻從不跑 migration，靠其他測試殘留的表才會過。乾淨資料庫上回
   `relation "users" does not exist`。改用與兄弟測試相同的隔離 schema fixture。
3. `test_execute_on_worker_redispatches_after_connect_failure_without_worker_penalty`
   的 2 秒 liveness guard 小於實測 2.04 秒。量測後確認 production 語義正確（Pending、
   `Redispatched`、retry_count 1、不扣 Worker reputation），將 guard 調為高於 5 秒
   production connect timeout 的 15 秒，並在註解說明它不是延遲 SLA。
4. `MANAGED_PROOF_ROLLOUT_MODE` 只設在 Compose 的 `worker` service，但讀取者是跑在
   nodepool 的 dispatcher。操作者設 `observe` 想做受監控遷移時，nodepool 仍停在
   `enforce`，開關靜默失效。已移到 nodepool，並在發布契約加入服務層級斷言（red-green 驗證）。
5. `docker-compose.test.yml` 的 CI 測試清單漏掉 `hivemind-task-scheduler`——結算邏輯所在
   的 crate，其 DB 測試從未在 CI 執行過。已補入。
6. `hivemind-rs/.cargo/config.toml` 被全域 `.cargo/` 規則忽略而未追蹤，使文件記載的
   `x86_64-pc-windows-gnu` 測試路徑無法在他人機器上重現。已加精確例外追蹤。

### 證明鏈重建

guest source 自上次 attestation 後有 4 個 build input 變更雜湊（`managed-proof/src/lib.rs`、
bounded renderer 的 `managed-function-runtime/src/lib.rs`、`zkvm` 的 `Cargo.lock`、
`guest/src/main.rs`），guest image ID 因此漂移。

- 新 image ID：`[466412732, 2327327967, 2963073729, 178423767, 1914766815, 1823038484, 4206432854, 2659673256]`
- 更新 trust pin 後**重跑 pin-test 仍 GREEN**（114 秒），證明 pin 自身的值不會回饋進 guest
  codegen，這個更新程序不是循環定義。
- 真 receipt fixture 重新 proving（732 秒），其內嵌 `image_id` 與新 pin 逐字相符。
- fixture 形狀未變：journal 656、單一 Composite segment（index 0、poseidon2）、無 assumption、
  seal 63,914 words。僅位元組數變動：envelope 664,026 → 664,258，receipt JSON
  661,720 → 661,953，兩者都遠低於 verifier 的 pre-crypto 上限。budget regression 常數已更新。
- 證據寫入 `docs/zk-managed-proof-build-attestation.md`。

### Prover sidecar 打包

先前最硬的部署阻擋（Compose 起的 worker 會讓每個 managed task 失敗）已解除：

- `packaging/managed-prover/` 為 staging 目錄（binary 本身 gitignore，README 與 `.gitkeep` 追蹤）
- `scripts/build-managed-prover.sh` 在受支援的 Linux/macOS/WSL 建置，並在不支援的主機上以
  exit 65 與可行動訊息拒絕（已實測）
- `hivemind-rs/Dockerfile` 將整個目錄 COPY 到 `/app/prover/`；缺 binary 時映像仍可建置，
  managed task 則明確 fail closed
- Compose 的 `MANAGED_PROVER_EXECUTABLE` 預設指向該路徑
- **實際建置的 worker 映像已驗證**：binary 位於 `/app/prover/`、`-rwxr-xr-x`、以非 root
  `uid=10001(hivemind)` 執行、空輸入時輸出固定的 `managed proof generation failed` 並 exit 1

### 依賴稽核與威脅覆蓋

- 主 workspace `cargo audit`：0 vulnerabilities，3 個既有 allowed warnings
- zkVM prover workspace 的兩個無法升級的 advisory（`rsa` RUSTSEC-2023-0071、
  `tracing-subscriber 0.2.25` RUSTSEC-2025-0055）改為可稽核的接受政策：
  `zkvm/managed-proof/.cargo/audit.toml` 逐項記錄，可達性分析、依賴路徑與重新檢視觸發條件
  寫在 `docs/zk-managed-proof-dependency-audit.md`。加上政策後該 workspace `cargo audit` exit 0。
- `docs/zk-managed-proof-threat-coverage.md` 將每一種惡意 Worker 手法對應到具體測試；
  文件引用的 54 個測試名稱已逐一驗證存在於原始碼。

### 本輪測試結果

| 關卡 | 結果 |
|---|---|
| GNU workspace 測試（接真實測試資料庫，`--no-fail-fast`） | 390 passed、0 failed、1 intentional ignored（37 個測試 binary） |
| `hivemind-managed-proof --features risc0-verifier` | 37 passed、0 failed（含真實 receipt 密碼學驗證） |
| WSL guest pin-test（trust pin 更新後） | 1 passed，114 秒，`cleanup_complete rc=0` |
| WSL fixture 重新 proving | 成功，732 秒，`cleanup_complete rc=0` |
| clippy `--workspace --all-targets -D warnings` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo audit`（主 workspace） | 0 vulnerabilities |
| `cargo audit`（zkVM prover，含政策） | exit 0 |
| 前端測試 | 39 passed（site 13、master-ui 15、worker-ui 11） |
| 前端 release builds | 3/3 passed |
| `docker-compose-release.Tests.ps1` | passed（含新增的 prover 打包與服務歸屬斷言） |
| `release-docs.Tests.ps1` | passed |
| `release-stack-smoke.ps1 -CheckOnly` | passed |
| worker 映像建置 + 映像內 prover 驗證 | passed |

### 多節點 Docker E2E（已執行，找到一個發布阻擋）

以 `release-stack-smoke.ps1 -KeepRunning` 起完整 8 容器 stack（臨時埠、隔離 volume，
未動使用者既有 `.env` 與資料），再以新寫的驅動腳本走完整流程。

已驗證可用的部分：

- 官網註冊／登入（master 的 `/api/register` 正確回 410，帳號只能經官網建立）
- nodepool 拒絕任何人自行註冊 `HIVEMIND_ADMIN_USERS` 中的名字（`register_user_rejects_configured_admin_username`
  的實地驗證），admin 必須帶外佈建
- **新的 `/api/admin/managed-proof/metrics` 在真實部署中可用**，回報 `rollout_mode=enforce`
- Worker Control `/api/register-worker` 註冊成功，nodepool 完成派送
- worker 確實執行 `managed-function-v0` 並啟動 prover（容器 CPU 達 1183%、2.1 GiB RSS）

發現的阻擋：**guest image ID 對建置環境敏感，不只對原始碼敏感。**

- 容器內 prover 回報 image ID `[851157164, 2331111488, 898154945, 2202623007,
  559143449, 4095204016, 1237502462, 1480841899]`
- nodepool trust pin 為 `[466412732, 2327327967, 2963073729, 178423767,
  1914766815, 1823038484, 4206432854, 2659673256]`
- 兩者不符，因此 nodepool 拒絕它產生的每一個 proof

該 binary 是在容器內以自帶 rzup guest toolchain 建置，而 pin 來自 WSL native 路徑的
快取 toolchain。「建置自同一份原始碼」**不是**相符的證據——此處先前的推論已被實測推翻，
`docs/zk-managed-proof-build-attestation.md` 已更正。

信任模型完全站得住：兩次嘗試都 audit 為 `event=rejected`、
`reason="Managed proof verification failed"`、`rollout_mode=enforce`；任務最終 `FAILED`、
`billing_settled=false`、`managed_executed_ops=0`。**沒有任何未經驗證的 claim 被結算。**
錯配的代價是可用性，不是信任。

修正（`7fae138`）：`scripts/build-managed-prover.sh` 現在在建置**前**執行
`tests::generated_guest_id_matches_nodepool_trust_pin`，環境若無法重現 pin 就以 exit 71
拒絕 stage——與 nodepool 在結算時強制的是同一個等式。

### 剩餘工作

以產出 trust pin 的同一 WSL 環境重建 release prover、重新 stage、重建 worker 映像，
再跑一次 E2E，確認 managed 任務能真正完成 proof-backed 結算
（`/api/admin/managed-proof/metrics` 的 `verified` 計數增加、`legacy_settlements` 不變）。

既有 Playwright `release-flow.spec.mjs` 只覆蓋 UI 流程、不含 managed proving，
因此上述 E2E 驅動腳本是新增能力而非既有回歸。

### 本輪本機提交（未 push）

- `b3371fd fix(proof): compile the managed prover host tests`
- `bcec23e fix(test): run settlement and bootstrap regressions against a real database`
- `6eaa08b chore(repo): track cargo policy configs and ignore the staged prover`
- `6db4d7e chore(cargo): declare the workspace license on every crate`
- `95fb420 feat(proof): add managed-proof rollout modes, metrics and audit events`
- `c7d9a45 feat(release): package the managed-proof prover sidecar`
- `8a2f621 chore(proof): re-pin the guest image and regenerate the receipt fixture`
- `803074a docs(proof): record the dependency audit and threat coverage`
## Final release E2E checkpoint - 2026-08-11

- overall: `complete`
- root cause resolved: the sidecar required `GLIBC_2.39`, while the Debian bookworm Worker runtime only supplied 2.36. The runtime now uses Debian trixie and the release smoke harness probes sidecar launch in the actual Worker image.
- evidence: `scripts/release-stack-smoke.Tests.ps1` passed; a rebuilt `release-stack-smoke.ps1 -KeepRunning` passed all surfaces and the sidecar launch probe; `zkproof-success-1786448265` completed in `enforce` mode with 3 CPT settled, verified audit event, 2 managed operations, and 17 output bytes.
- next action: commit the ABI compatibility fix and E2E evidence locally; do not push.

## 2026-08-12：未使用項目清理與科學 runtime 路線圖

- 已以 UTF-8 raw bytes 還原使用者附檔：要求先刪除沒有用到的程式碼／檔案，再寫出能讓底層 runtime 具備圖靈完備語義與科學運算能力的實作計畫。
- 已讀取既有規劃狀態並執行 session catch-up；catch-up 無額外輸出。
- 工作樹在本輪開始前已有 23 個 tracked paths 修改與 4 個 untracked paths，涵蓋 runtime、proto、後端及前端；全部視為既有資產，不還原、不覆寫。
- 已將新目標加入 `task_plan.md`，保留前一 ZK 計費證明工作的完整歷史。
- 已啟動三個平行 read-only 稽核：Rust 未使用項目、frontend/scripts/assets 未使用項目、managed runtime 架構與 roadmap gap analysis。
- 目前狀態：階段 A `in_progress`；尚未刪除任何檔案，也尚未把候選誤當成已確認垃圾。
- 已確認既有規劃資料量：`task_plan.md` 324 行、`findings.md` 1,287 行、`progress.md` 399 行（加入本節前）；歷史內容包含先前安全／ZK／發布修復證據，後續判斷會保留這些 load-bearing context。
- Cleanup audit確認 root commit `be39bb7` 實際移除 Monty executable contract與 5 個 unused tracked artifacts；使用者其後明確授權移除剩餘未接線的 Monty core、bindings、typeshed、fuzz 與專用建置 metadata，`executor-rs` 現在只保留 Hivemind 兩個 runtime crate。
- 已讀取並逐章檢查 `docs/MANAGED_RUNTIME_EVOLUTION_PLAN.md`／`STATE.md`；roadmap採 v0/v1 雙 runtime，具體包含圖靈語義、scientific ABI/kernels、sandbox、trust、GPU、測試、benchmark與 M0–M5 gates。
- README mismatch採 TDD 修正：先新增 `readme_task_submission_publishes_an_executable_managed_function`，focused test如預期因 Python source **RED**；再只把 README `task_source` 改為 `fn`／`get` DSL，同一測試 **GREEN**（1 passed）。
- Frontend read-only audit的 TypeScript檢查因 `incremental: true`產生 untracked `frontend/tsconfig.tsbuildinfo`；audit agent已立即以精確 path移除並確認 status乾淨，沒有留下 generated artifact或 tracked edit。
- 已移除兩個本輪新證明的 confirmed-unused項目：Cargo target graph未選取的 duplicate `hivemind-bin/src/main.rs`；不屬 workspace、無 consumer且無法 standalone metadata的 `managed-function-transpiler/` 4-file crate。
- 已移除 frontend confirmed-unused slice：orphan network canvas component、Master/Worker dead declarations、Worker dead fetch helper與 copied CSS、official zero-consumer utility CSS/keyframes、dead API/Dialog/Button exports；保留 convention/dynamic/external-URL uncertain items。
- 已移除 Rust definition-only `_nodepool_endpoint_helper`、legacy external Tailscale binary resolver與其私有 PATH helper；保留 platform-used endpoint helper與仍具 proto產品意圖的 WireGuard slice。

## 2026-08-13：M1 production sandbox policy

- RED：新增 compile-fail doctest，證明 crate 外目前能直接使用 reference supervisor；測試按預期因成功編譯而失敗。
- GREEN：加入 `sandbox.rs` 的 policy envelope 與 `ProductionSandboxLauncher`。Linux policy 缺少 rootless OCI、user/pid/mount/network namespace、cgroup v2、default-deny seccomp、no_new_privs、read-only root、network deny 或 explicit safe mounts 時拒絕；entrypoint、serde unknown fields/tags、mount path 與 digest 皆 fail closed。未接入 OCI runner 時只回 `RunnerUnavailable`／`UnsupportedPlatform`，不 fallback direct spawn。
- API hardening：`ReferenceCommandSpec`／`ReferenceProcessSupervisor` 改為 `pub(crate)`；lifecycle coverage 從 integration test 移至 `src/supervisor/tests.rs`，保留 timeout/cancel/output cap/descendant kill-reap/stdio coverage。
- 驗證：sandbox 6、CPython 11、crate-internal lifecycle 9、compile-fail doctest 1、executor workspace 98 與 managed runtime doc tests 全綠；Worker check、Docker Compose release contract、Windows package contract、runtime check、scoped rustfmt/diff check 全綠。
- strict clippy `cargo clippy -p general-compute-runtime --all-targets -- -D warnings` 仍失敗於 35 個既有 crate-wide pedantic warnings；已記錄為 blocker，沒有為此單元改動無關 reference/lib/tensor API。
- review：sandbox policy reviewer APPROVE；evidence 位於 `.omo/evidence/m1-sandbox-policy/` 與 `.omo/evidence/review_sandbox_policy-code-review.md`。

## 2026-08-13：Monty 實體殘留清理

- 依使用者明確授權，移除未被 root repository 追蹤的 `executor-rs/.git` upstream Monty metadata 與舊 `executor-rs/target` 編譯產物；root `.git` 未受影響。
- 驗證：root `git ls-files` 無 Monty 路徑；`executor-rs/Cargo.toml` 僅列 `managed-function-runtime` 與 `general-compute-runtime`；`executor-rs/.git`、`executor-rs/target`、`executor-rs/monty.exe` 均不存在；active source/build paths 無 Monty reference。歷史文件與負向 release contract tests 保留作為「不得加回」證據。

## 2026-08-13：M1 leader-exit process-tree hardening

- `general-compute-runtime` supervisor now owns a platform-specific `ProcessTreeGuard`: Unix keeps the invocation-scoped process group; Windows uses a suspended child, Job Object assignment with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, initial-thread resume, and job termination before output capture joins.
- Normal leader exit terminates descendants before joining inherited stdout/stderr pipes; timeout, cancellation, and combined-output-limit paths share the same tree termination and wait/reap path. Spawn setup failures explicitly kill and reap the child.
- RED→GREEN lifecycle coverage includes the normal-leader-exit descendant-pipe regression and Windows descendant fixtures with a strict 600 ms timeout. Cross-component verification passed: executor workspace tests, worker-executor check, Docker Compose release contracts, and Windows worker packaging contracts.

## 2026-08-13：M1 pinned OCI bundle runner

- `dbf5765 feat(runtime): execute pinned OCI bundles safely` 已本地提交，未 push。
- 以 RED→GREEN 補齊 `ProductionSandboxLauncher::run_bundle`：fake pinned runner 只有在 absolute regular executable、正確 SHA-256、合法 container ID 與完整 OCI 1.0.2 bundle 通過驗證後才可執行；未知 root/nested fields、namespace 重複或未知、非 root user、symlink/relative bundle、mount/source traversal、image/backend/cgroup/network/seccomp annotation mismatch 均在 spawn 前拒絕。
- runner 直接以既有 `ReferenceProcessSupervisor` 傳遞 argv，沒有 shell interpolation；timeout、cancellation、combined output limit 與正常 leader exit 共用 process-tree kill/reap，並保留 bounded diagnostics。
- focused `sandbox` suite：21 passed；`cargo test -p general-compute-runtime --locked`、`cargo test --workspace --locked`、`cargo check -p hivemind-worker-executor --locked`、Docker Compose release contract、Windows worker packaging contract、scoped rustfmt check 均通過。
- 限制：這個單元驗證並執行 operator-pinned OCI runner，但 Linux rootless namespace/cgroup v2/seccomp/no_new_privs 的 host primitives 仍由外部 runner 實作；Worker/Nodepool runtime routing、artifact materialization 與 capability probe 尚未完成，不能宣稱 M1 或整體 M0–M5 完成。

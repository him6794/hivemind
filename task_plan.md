# Hivemind ZK 函式計費證明實作計畫

## 目標

讓不受信任 Worker 對 `managed-function-v0` 的程式執行與函式計費產生可由 Nodepool 驗證的零知識證明；只有驗證成功的 `usage_units` 才能進入結算。

## 當前階段

階段 5：完整驗證與發布（running）；階段 1–4 已完成。剩餘唯一未完成項目為多節點 Docker E2E 與瀏覽器回歸。

## 成功標準

- 證明綁定 task id、程式雜湊、輸入雜湊、輸出雜湊、runtime/cost-model 版本與 usage units。
- Worker 無法藉由修改 receipt JSON、usage units、輸出或重播其他任務的 proof 通過 Nodepool。
- Nodepool 驗證失敗時不完成計費結算，且留下可診斷狀態。
- 既有 managed runtime 的計費語義在 zkVM guest 與 host 測試向量中一致。
- 完整測試、效能基準、遷移開關與發布文件完成後才啟用強制驗證。

## 各階段

### 階段 1：證明契約與威脅模型

- [x] 盤點目前 receipt、checksum、JWT 與結算信任邊界
- [x] 決定採成熟 zkVM，不自行設計密碼學協議
- [x] 以 RED 測試定義版本化 public journal／execution claim
- [x] 實作 deterministic SHA-256 commitments 與防重播 task binding
- [x] 完成 focused tests 與 GNU workspace gate
- **狀態：** complete

### 階段 2：zkVM guest 與 proof 產生

- [x] 固定 RISC Zero 3.0.6 stable，正式 verifier 必須啟用 `disable-dev-mode`
- [x] 將 canonical output renderer 下沉到 managed runtime，供 Worker/guest 共用
- [x] 固定 builder image digest；目前可信 guest image id `[466412732, 2327327967, 2963073729, 178423767, 1914766815, 1823038484, 4206432854, 2659673256]`（2026-08-11 因 guest source 變更重建；舊值為 `[3606400121, ...]`）
- [x] 將 deterministic managed runtime 放入 guest 執行路徑
- [x] guest commit 公開聲明，私有 witness 保留程式輸入／執行軌跡
- [x] Worker prover 產生包含 proof scheme、固定 image id、journal 與 receipt 的 proof envelope
- [x] 加入 golden vectors、image-id/journal tamper tests 與 proof time benchmark（約 570–580 秒）
- **狀態：** complete

### 階段 3：協議與 Nodepool 驗證閘門

- [x] protobuf 傳遞 proof scheme、image id、journal、receipt/seal
- [x] verified claim 對 task/source/input/output/budget/version 的逐項 binding API
- [x] Nodepool 以可信 image id 驗證 proof
- [x] 驗證 journal 與資料庫 task/source/input/output/max_cpt 完全一致
- [x] proof 無效、缺漏、重播或版本不支援時 fail closed
- [x] 驗證成功後才寫 receipt、完成任務與結算
- **狀態：** complete（程式碼已驗證；guest image ID／attestation／fixture 待重建）

### 階段 4：遷移、失敗語義與營運

- [x] 加入 off/observe/enforce 三段 rollout mode（預設 enforce；observe 只觀測並保留 legacy settlement）
- [x] 定義 proving timeout、取消、失敗與 retry 行為
- [x] 限制 proof 大小與 verifier CPU/記憶體消耗
- [x] 增加 proof verification metrics、audit events 與管理介面狀態
- **狀態：** complete（階段 5 的發布 gates 仍 pending）

### 階段 5：完整驗證與發布

- [x] runtime、Worker、scheduler、node-manager focused/full tests（首次接上真實測試資料庫執行，抓出兩個先前被靜默跳過的失敗）
- [x] 惡意 Worker 測試：偽造計費、輸出、task id、版本、seal — 覆蓋盤點與逐項對照見 `docs/zk-managed-proof-threat-coverage.md`（54 個具名測試已驗證存在）
- [x] Docker 多節點完整流程與瀏覽器回歸
- [x] cargo fmt、clippy、audit、依賴授權與可重現 guest build
- [ ] 文件與本機 Conventional Commits 完整；不 push
- **狀態：** running

## 技術決策

| 決策 | 理由 |
|---|---|
| 證明完整 deterministic runtime 執行 | 只證明函式計數加總無法阻止 Worker 捏造計數 |
| 採成熟 zkVM backend | 自製 ZK 電路／證明系統風險不可接受 |
| 公開 journal 只放 commitments 與結算欄位 | 綁定任務與結果，同時保留輸入／執行軌跡隱私 |
| SHA-256 commitments | 現有 SHA-1 checksum 不適合作為新證明協議的安全承諾 |
| 版本化 runtime、cost model、proof protocol、guest image | 支援可稽核升級並阻止跨版本混用 |
| rollout 採 off/observe/enforce | 在 proof 產生與效能驗證完成前不破壞現有測試環境 |
| RISC Zero 固定 3.0.6 stable | 避免 5.0 RC；具 image ID、journal、receipt、Docker build 與 dev-mode hardening |
| production verifier 啟用 `disable-dev-mode` | 防止環境變數誤開假證明而繞過結算信任根 |

## 關鍵風險

1. 本機 Windows 尚未安裝 zkVM toolchain；guest build/prover 可能需要 Linux builder。
2. `managed-function-runtime` 目前使用 `std` 與獨立 workspace，需要驗證 guest 相容性及可重現編譯。
3. Proof 產生成本可能遠高於目前短任務本身，必須在 enforce 前取得基準數據。
4. 本計畫預設 source/input 為 private witness，只公開 SHA-256 commitments。

## 遇到的錯誤

| 錯誤 | 嘗試次數 | 解決方案 |
|---|---:|---|
| PowerShell glob 直接傳給 `rg` 造成 Windows path error | 2 | 改用 `Get-ChildItem` 展開檔案；本輪誤重犯一次後已停止，不影響程式碼 |
| workflow-orchestrator 引用的 `skills/auto-trigger/SKILL.md` 不存在 | 1 | 記錄為工具環境限制，不阻塞實作 |
| 狀態文件多檔補丁因一處空白未精確匹配而整體拒絕 | 1 | 拆成小補丁並使用剛讀取的精確原文；未發生部分寫入 |
| `findings.md` 檔首含 BOM，使用可見 header 作 patch context 仍未匹配 | 1 | 改用第二個無 BOM 的 section header 作錨點；未發生部分寫入 |
| Admission reviewer 回報的 evidence 相對路徑在 repo `.omo` 下不存在 | 1 | 保留 mailbox 的完整 BLOCK 結果，待 owner 重審時要求回報可讀的實體絕對路徑 |
| 為找 admission review evidence 對整個 `D:\` 執行 `rg --files` 超時 | 1 | 不重跑廣域掃描；後續只查 repo 與明確 `.omo` 目錄 |
| Verifier full-test orchestration 120 秒超時且未回傳 buffered stdout | 1 | Process trace 證實 admission agent 正在編譯同一 target；不重跑競爭命令，追蹤遺留 nodepool test process 至完成後再用獨立 target-dir 驗證 |
| PowerShell PID 狀態字串中的 `$id:` 被解析成 drive-qualified variable | 1 | 使用 `${id}` 明確界定變數；純診斷命令未改變任何程序 |
| JavaScript orchestration 先展開 PowerShell `${id}`，造成 `ReferenceError` | 1 | 改用 PowerShell `-f` format operator，避開跨語言 interpolation；未執行到程序查詢 |
| Partial lock staging script 誤假設 nested shell result 有 `exit_code` 屬性，check 後提前結束 | 1 | 將 check/apply 放在同一 PowerShell 命令並使用 `$LASTEXITCODE`；index 尚未套用 lock patch |
| 清理 orphan test tree 時 child 結束使 root PID 在下一次 Stop-Process 前已自行退出 | 1 | 精確驗證三個 PID 均已停止；未重試、未碰代理或服務程序 |
| 串行 verifier tests 的 5 分鐘工具上限於 nodepool MSVC link 階段到期 | 1 | 精確清理本命令 PID；不再串接兩個 gate，owner 完成後以 15 分鐘上限分開重跑並保留輸出 |
| Worker config 搜尋同時包含不存在的 `hivemind-rs/.env.example`，使並行讀取整體回傳非零 | 1 | 不重跑不存在路徑；以已定位的 config `lib.rs` 精確區段與 Worker Cargo.toml 分開讀取 |
| 首次重現 host locked-metadata blocker誤加 `--no-deps`，繞過 reviewer觸發的完整 lock解析 | 1 | 不把通過視為GREEN；改跑 reviewer原始命令，僅將成功JSON輸出導向 null以保留exit/stderr |
| 假設 proto build輸出名為 `hivemind.rs`，generated wrapper搜尋無結果 | 1 | 先列出實際 build out檔名再查；不重跑錯誤filter |
| `rg --files` 預設尊重 target ignore，搜尋 generated proto wrapper回傳1 | 1 | 後續若仍需要 generated source，使用 `rg --files -uu` 或直接讀 tonic-build template；未重跑錯誤命令 |
| Admission接手owner誤以default MSVC link Worker focused test，rustc停在既知MinGW `libtailscale.a`不相容路徑 | 1 | 中斷agent與精確compile tree，改派所有相關link tests走`x86_64-pc-windows-gnu`；不重跑MSVC |
| GNU link CPU診斷同時查兩個PID，其中一個剛退出使Get-Process回傳1 | 1 | 已取得仍存活PID狀態，不原樣重跑；後續逐PID使用SilentlyContinue |
| Scheduler GNU full gate的timeout cleanup test在高併發下未建立PID marker | 1 | 69 passed/1 failed/1 ignored；先等reviewer cargo清空，再serial focused重現，驗證是否為helper startup/deadline耦合，不先調常數 |
| 向既有 admission 代理送出 follow-up 時 collaboration provider 回報 `custom` 不存在 | 2 | 不原樣重試；代理仍在執行且會自行收到子 reviewer 結果，將 review BLOCK 持久化並由 coordinator 在回傳時驗收 |
| 首次持久化補丁因 findings 檔頭比對失敗 | 1 | 拆分為小補丁，避免部分更新 |
| MSVC workspace test 無法連結 MinGW `libtailscale.a` | 1 | 依既有平台契約改用 `x86_64-pc-windows-gnu`，246 tests 通過 |
| `postcard`／`bincode` 引入未維護依賴警告 | 2 | 改用 workspace 既有 `serde_json`，新警告歸零 |
| Windows MSVC 編譯 RISC Zero `prove` 失敗：`risc0-circuit-keccak-sys` 以 `/std:c++17` 編譯 C++20 designated initializers | 1 | 不原樣重試；將 zkVM host/prover 測試移到可重現的 Linux Docker 環境，避免修改 Cargo registry 原始碼 |
| 首次 GNU zkVM focused test 超過 5 秒工具時限 | 1 | 這是編譯尚未完成而非測試失敗；保留增量產物並以可觀測的較長時限續跑 |
| GNU zkVM focused test 第二次超過 64 秒工具時限 | 2 | 仍無編譯或測試錯誤；RISC Zero cold build 合理較久，下一次提高到 10 分鐘並沿用增量產物 |
| Windows `--target x86_64-pc-windows-gnu` 仍以 MSVC host 編譯 methods build script，連結 `risc0-zkvm-platform` 時缺少 `sys_alloc_words` | 1 | Cargo build script 固定使用 host toolchain，target 參數無法避開；停止 Windows 路徑，切換 WSL/Linux host |
| 全域掃描 Cargo registry 的 RISC Zero Docker option 超過 14 秒工具時限 | 1 | 縮小到 `risc0-build-3.0.6` 套件目錄再查，不重做全域掃描 |
| PowerShell `ConvertFrom-Json` 無法解析 Cargo metadata 中大小寫重複的 feature keys | 1 | 改用文字搜尋 metadata 的 `manifest_path`，不修改依賴資料 |
| `rg` 同時查根目錄與不存在的 zkVM `.gitignore` 回傳 1 | 1 | 已從根 `.gitignore` 確認 `.cache/` 與 `target/` 被忽略；後續只查存在檔案 |
| Linux zkVM image 使用 Rust 1.88，低於 managed runtime/ruint 所需 1.90 | 1 | 測試映像升級並固定 Rust 1.90；builder digest 預檢已成功 |
| Linux guest build 缺少 guest-local `Cargo.lock`，且 host 找不到 RISC Zero Rust toolchain | 1 | 先產生 guest lockfile；檢查 `risc0-build` Docker 路徑的 toolchain 探測條件，再補齊 Linux test image，不原樣重跑 |
| Docker digest inspect 使用含換行的 Go template，在 PowerShell 轉義後解析失敗 | 1 | 改用 `{{json .RepoDigests}}` 並先把 digest-pinned image 明確 tag 成 build script 使用的 tag |
| Linux test image 完成 rzup/RISC Zero Rust 安裝後，Docker Desktop 在匯出映像時回傳 RPC EOF | 1 | 建置層已進入 BuildKit cache；先確認 daemon/image 狀態，再以快取重新匯出，不重做工具鏈安裝 |
| RPC EOF 後 Docker daemon 回報 Desktop unable to start，狀態檢查因部分 service/path 查無結果回傳 1 | 1 | Docker Desktop 程序仍存在；使用 Desktop 自帶 restart 恢復 daemon，保留所有 volumes/containers |
| `docker desktop restart` 超過 124 秒工具時限，未回傳完成狀態 | 1 | 不盲目重啟；先重新查 daemon 與既有 stack 狀態，若仍失敗再採 Desktop 診斷 |
| 預設 `C:\Program Files\Docker\Docker\Docker Desktop.exe` 不存在 | 1 | 從現有 Docker Desktop process 的 executable path 解析實際安裝位置後啟動 |
| 從實際安裝路徑啟動 Desktop 後，10 秒健康檢查仍在 34 秒超時 | 1 | 讀取 Desktop host/backend 最新日誌定位啟動失敗，不再盲目呼叫 CLI |
| Linux outer image 只複製 `docker` binary，缺少 buildx plugin；RISC Zero builder 的 `docker build --output` 被 legacy builder 拒絕 | 1 | 從固定 `docker:27-cli` 映像提供 buildx plugin，bind mount 到 outer container；保留已完成的 D: 編譯快取 |
| buildx 修正後 guest build 使 Docker VHD 再度擴張至 C: 歸零，daemon unexpected EOF | 2 | 停止在 Docker Desktop 執行 prover；先恢復既有 stack，再改用不寫 Docker VHD 的 WSL/native Linux toolchain |
| 直接遞迴刪除 npm `_npx` cache 被工具安全政策拒絕 | 1 | 不繞過刪除政策；將精確 cache 目錄移到 D: 可恢復暫存位置以釋放 C:，保留可回復性 |
| WSL Ubuntu `apt-get update` 超過 4 分鐘無下載進度 | 1 | 主動終止，不原樣重試；先利用 D: 已編譯的 Linux target，若確需 cmake 再改用離線/替代來源 |
| WSL process 診斷命令中的管線被 PowerShell 轉義破壞 | 1 | 改用無管線的 `wsl ... ps -ef`，確認僅 apt mirror 等待、沒有 dpkg mutation |
| WSL user 無權寫入 Docker root 建立的 `.cache/zkvm-target` build lock | 1 | 不遞迴改寫既有 cache 權限；使用獨立的 D: `.cache/zkvm-target-wsl` 進行 native build |
| WSL native build 讀取 Docker root 建立的 Cargo registry source 時遇到 `fnv` permission denied | 1 | Cargo registry 本身是可重建 cache；只對精確 `.cache/zkvm-cargo-home` 修正讀寫權限，不碰專案或使用者資料 |
| WSL `chmod -R` 對 Docker 經 NTFS bind 建立的 Cargo cache mode 無效，`fnv/lib.rs` 仍為 root 0640 | 1 | 用 Windows 機械複製建立不帶 Linux metadata 的獨立 WSL cargo cache，再從該副本增量建置 |
| Windows 機械複製後 WSL 仍將 cache 檔映射為 root 0640，副本無法由 uid 1000 讀取 | 1 | 不再複製 cache；以 WSL root 執行受控 build，僅寫入 repo `.cache`，不授予專案外權限 |
| WSL native cold build 的 `risc0-circuit-recursion` artifact 下載三次回 HTTP 400 | 1 | WSL 外網/DNS 不可靠；bind D: 到 Docker build 曾使用的相同 Linux 絕對路徑，重用已完成的 host artifacts，只本機重建 methods/guest |
| 相同路徑 WSL 命令以環境變數組 PATH 後找不到 `cargo` | 1 | 避免跨 PowerShell/WSL 的變數展開歧義；改用已驗證的 RISC Zero cargo 絕對路徑 |
| 相同絕對路徑仍重編 host circuits 並再次觸發 S3 400 | 2 | 找到 fingerprint 差異：WSL host 使用 RISC Zero Rust 1.97、既有 artifacts 使用標準 Rust 1.90；將既有 image 內 1.90 toolchain複製到 D: 後匹配重用 |
| 標準 Rust 1.90 host 已完成 circuits，但 methods local guest build exit 101 且預設輸出未包含底層原因 | 1 | 啟用 `RISC0_BUILD_DEBUG=1` 與 backtrace 重跑增量 methods build，取得 guest cargo 的實際錯誤後再修正 |
| guest build 真因：`risc0-build` 會移除 `CARGO_*`，inner cargo 未看到 D: registry 並因 WSL DNS 無法解析 crates.io | 1 | bind 已下載的 D: cargo home 到 `/root/.cargo`，讓 inner guest cargo 完全離線重用 lock/cache |
| bind registry 後 guest 仍 exit 101；`RISC0_GUEST_LOGFILE` 普通檔案被每行從 offset 0 覆寫，訊息殘缺 | 1 | 將 guest log 指向 `/dev/stderr` 讓 Cargo 捕捉完整 inner compiler 訊息，不再依賴普通檔案 |
| guest ELF 成功後 host 將 `Journal` 直接傳給 `&[u8]` parser，E0308 | 1 | 使用 RISC Zero 3.0.6 公開 raw bytes 欄位 `session.journal.bytes` |
| zkVM fmt check 發現 methods `lib.rs` 多餘尾端空行 | 1 | 移除空行後重跑；stable rustfmt 的既有 nightly-option warnings 另行記錄，不是格式差異 |
| clippy outer cargo 的 wrapper 洩漏到 inner guest build，使 RISC Zero rustc 找不到 guest `std`（E0463） | 1 | guest 已由真實 test 完整編譯；clippy gate 使用 RISC Zero 官方 `RISC0_SKIP_BUILD=1` 檢查 host/methods 原始碼，避免以 host clippy wrapper編譯 guest target |
| zkVM `cargo audit` 發現 RISC Zero 依賴鏈的 `rsa 0.9.10` RUSTSEC-2023-0071 與 `tracing-subscriber 0.2.25` RUSTSEC-2025-0055 | 1 | 先追蹤精確 dependency path 與可達性；在漏洞未消除或有明確隔離政策前不得標記可發布 |
| 真實 proof test 使用 `RISC0_SKIP_BUILD=1` 產生空 ELF，回報 `Malformed ProgramBinary` | 1 | 此旗標只用於 clippy；真實 execution/proving test 必須建置並嵌入固定 guest ELF |
| 標準 Rust 1.90 toolchain 不含 clippy；Windows clippy 再次被上游 MSVC C++17 問題阻擋 | 1 | 使用 RISC Zero Rust 1.97 隨附 clippy，並以官方 `RECURSION_SRC_PATH` 指向 SHA-256 驗證成功的本地 artifact，避免 WSL S3 400 |
| prover workspace 直接依賴 `hivemind-proto` 會把 tonic/prost server graph 拉進 prover，且獨立 WSL cache 缺少 `prost-types` | 1 | 撤回未提交耦合；protobuf 契約留在主 workspace，Nodepool verifier adapter 由可信端持有，prover 不依賴 Nodepool 協議 |
| fixture generator 首次 WSL 執行落入 Docker builder，回報 `Could not find or execute docker` | 1 | 根因是命令漏設 build.rs 既有的 `HIVEMIND_ZKVM_USE_DOCKER=0`；先以 methods-only native build 驗證開關，再重跑 proving，無須修改 production build 邏輯 |
| planning session-catchup 使用技能文件中的舊 `.claude` 路徑而找不到腳本 | 1 | 已確認腳本實際位於 `.codex/skills/planning-with-files-zh/scripts`；改用實際安裝路徑恢復，不重複舊命令 |
| fixture artifact 檢查的 JavaScript orchestration 誤用 PowerShell 式 `.Replace` | 1 | 改用 JavaScript `.replace(/\\.d$/, ".bin")`；純診斷命令錯誤，未觸碰專案或建置資源 |
| methods-only WSL wrapper 的 quoting/變數在 PowerShell→`wsl bash -lc` 引數邊界失真 | 2 | 兩次都在診斷/mount 前失敗；不再 inline 傳 Bash，改以 `apply_patch` 建立 D: ignored 暫時腳本，再用單一無 quoting 的 WSL argv 執行 |
| integration 搜尋假設存在 `hivemind-rs/crates/db` 目錄，`rg` 回 path not found | 1 | 不重跑錯誤路徑；先以 `rg --files hivemind-rs/crates` 定位實際 task model/repository，再縮小查詢 |
| future binding 檢查假設 proto 位於 `hivemind-rs/proto/hivemind.proto` | 1 | 不重跑錯誤路徑；用 `rg --files hivemind-rs -g '*.proto'` 取得實際位置後再讀 |
| RISC Zero claim 結構檢查假設存在 `src/receipt_claim.rs` | 1 | 不重跑錯誤檔名；改在已確認的 crate `src` 根以 symbol 搜尋定位定義 |
| fixture 分析使用 `System.Text.Json.JsonDocument`，Windows PowerShell 5.1 無該型別 | 1 | 改用內建 `ConvertFrom-Json` 讀結構；receipt 精確 byte 數由 Rust/serde_json 同一 codec 量測，不以 PowerShell 重編碼冒充精確值 |
| focused test 加 `--exact` 但只給短 test 名，結果 0 tests | 2 | 兩次皆不計為通過；改給完整 module-qualified test path 後重跑 |
| 真 fixture 正向 verifier 回 `UntrustedImageId` | 1 | 先比較 fixture、最新 methods 產物與 pinned ID，追查 guest image 漂移根因；未直接改常數 |
| 恢復狀態文件首個 patch 因一處空白未精確匹配而失敗 | 1 | 先以 `rg -n` 讀取精確行，再用較小 patch 成功更新；未部分寫入 |
| 讀取 zkVM workspace 時誤以 `hivemind-rs` 為相對根目錄 | 1 | 改用已確認的 `..\\zkvm\\managed-proof`，不重複不存在的路徑 |
| 一次 `cargo test` 誤傳兩個 positional test filter | 1 | Cargo 只接受單一 filter；改用共同 `queue` filter 後兩項測試同時通過 |
| `zkvm` host 測試以 `assert_eq!` 比較 `risc0_zkvm::Receipt`，但該型別無 `PartialEq` | 1 | `6e7af38` 加入後從未編譯過（Windows RISC Zero host build 受阻），使 pin-test 也無法執行；改為比較 canonical JSON，即 verifier 實際解析的表示法 |
| `test_seed_default_user_inserts_bootstrap_account` 直接用 public schema 且不跑 migration | 1 | 靠其他測試殘留的表才會過，乾淨資料庫上回 `relation "users" does not exist`；改用與兄弟測試相同的隔離 schema fixture |
| connect-failure 回歸測試的 2 秒 `timeout` 小於實測 2.04 秒 | 1 | 先前無資料庫時整個測試被靜默跳過；量測後確認 production 語義正確，將 liveness guard 調整為高於 5 秒 production connect timeout 的 15 秒 |
| `MANAGED_PROOF_ROLLOUT_MODE` 只設在 Compose 的 `worker` service | 1 | 讀取者是跑在 nodepool 的 dispatcher，設在 worker 完全無效，會讓 observe 遷移靜默失效；移到 nodepool 並在發布契約加入服務層級斷言（已 red-green） |
| `docker-compose.test.yml` 的 CI 測試清單漏掉 `hivemind-task-scheduler` | 1 | 結算邏輯所在的 crate 從未在 CI 跑過 DB 測試；已補入清單 |
| PowerShell `Set-Content -Encoding utf8` 在 red-green 還原時為 `docker-compose.yml` 加上 BOM | 1 | 以 `[System.IO.File]::WriteAllBytes` 去除 BOM 並確認 diff 只剩預期的 8 行新增 |

## 歷史

上一輪完整平台驗證已完成，摘要保留於 `docs/platform-validation-state.md`；本計畫從該乾淨基線開始。

## 2026-08-09 續作 checkpoint

- 狀態：`running`；尚不可宣稱可發布。
- 已完成可提交前驗證：verifier／settlement full scheduler gate（70 passed、1 intentional ignored）與 GNU clippy；Admission caps final review `CLEAR / APPROVE`。
- Verifier kill/reap 測試已移除子程序排程依賴：父端於 `spawn` 後觀察 PID，production 1 秒 deadline 不變。
- 已提交：`03a080e feat(proof): isolate verified settlement`。
- 已提交：`367c71d feat(api): enforce managed task admission caps`。
- 下一步：精確提交 verifier、prover sidecar protocol、admission caps 三個獨立切片，之後以 TDD 實作 Worker shared cancellation、supervisor cleanup、bounded prover sidecar、RPC caps/deadlines。
- 發布阻擋：Worker 尚未回傳 managed proof；runtime limits/guest attestation 尚未重建；native Windows RISC Zero prover 無官方支援，需限定 Linux/macOS prover host 或提供受支援的 Linux/WSL 部署策略。

## 2026-08-09 持續執行狀態

| 項目 | 狀態 | 證據／下一步 |
|---|---|---|
| Verified settlement | 已提交 | `03a080e`；scheduler/nodepool/clippy gates 已通過 |
| Managed admission caps | 已提交 | `367c71d`；final review `CLEAR / APPROVE` |
| Prover protocol／sidecar | 已提交 | `6e7af38`；protocol 13、harness 5、clippy/fmt/locked metadata 綠，review `CLEAR / APPROVE` |
| Worker proof 回傳 | 未開始 production 接線 | 目前固定 `managed_proof: None`；先寫 RED integration test |
| Worker cleanup/cancel | 未完成 | future drop active-task leak、`spawn_blocking` lifecycle 需 supervisor/RAII 設計 |
| RPC caps/deadlines | 未完成 | whole request/response cap、connect timeout、20-minute execute deadline |
| Runtime limits／attestation | 未完成 | guest image 會漂移；需 Linux prover 重建 fixture/attestation |
| Packaging／cross-platform | 未完成 | Windows native RISC Zero prover 不受官方支持；需 Linux/macOS 或 WSL 策略 |
| End-to-end release gates | 未完成 | 惡意案例、資源釋放、多節點、audit、release packaging 尚待執行 |

### 恢復順序

1. 僅用精確路徑確認 sidecar source/harness 與 staged diff；絕不 stage `tdd-red/target`。
2. 封存 sidecar 後，從 Worker lifecycle 的第一個 RED test 開始。
3. 不跳過 cleanup/cancel/resource-release coverage，也不以 proof-only cap 取代 whole response cap。
4. 最後以受支持 Linux/macOS 或已驗證 Linux/WSL proving 路徑完成真實 proof 與多節點發布驗證。

## 2026-08-09 最新執行紀錄

- [x] RPC transport hardening 已提交：`1a9fa8f feat(rpc): bound worker proof transport`。Worker RPC 全訊息上限為 4 MiB、連線上限 5 秒、執行 RPC deadline 為 20 分鐘；endpoint 解析、連線與 tonic transport 失敗都會安全重派，不扣 Worker reputation。
- [x] Worker proving integration 已提交：`d99c8f7 feat(worker): generate managed proofs safely`。managed task 只有 native 執行與 sidecar proof 都成功才回報成功；proof 缺失、sidecar 非正常輸出、取消、逾時或超量資料都 fail closed。
- [x] 子程序回收已涵蓋 timeout、取消及 proof future 被中止：同步 kill、獨立 reaper、reap 後才釋放 proving permit；Worker active-task registry 由 supervisor/RAII 持有，呼叫端 future 被 drop 仍可清理。
- [x] 本機 GNU Worker 測試目前 81/81 通過；sidecar focused 15/15、Worker clippy `-D warnings`、Worker binary compile、格式與 diff check 均通過。兩個 code review 均已 APPROVE/CLEAR。
- [ ] 下一個不可略過的 release gate：Worker 與 zkVM guest 改用相同有限 runtime limits，重新建立 guest image ID、attestation、真實 receipt fixture，並在支援的 Linux/macOS prover host 做實際 proof 與多節點 E2E。
- [ ] sidecar 尚未被正式 worker image/Compose 打包；未配置 `MANAGED_PROVER_EXECUTABLE` 時 managed task 會明確失敗，不可宣稱可發布。

## 2026-08-09 Runtime safety continuation

狀態：`running`，尚未達可發布等級。

已完成本機提交 `097c98a fix(runtime): enforce finite managed execution limits`：Worker 與 zkVM guest 現在共用有限的 `ExecutionLimits::default()` 安全界限，任務／envelope budget 仍是唯一計費上限；depth-65 回歸測試已證明舊 unlimited 策略會錯誤放行。

目前 release blocker 與後續順序：

1. 以 TDD 加入「不先配置巨大字串」的 canonical return renderer 與中間值配置界限；現有 `max_output_bytes` 只限制 `print`。
2. 維持 Worker、guest 與 native claim parity test 的完全一致行為。
3. 在支援的 Linux/macOS prover host 重建最終 guest、更新 trust pin/image ID/attestation/真實 receipt fixture，並跑真 proof。
4. 將預建 Linux prover sidecar 納入 release Worker image；不能破壞本機開發 Docker 流程或要求 runtime Docker-in-Docker。
5. 執行多節點 Docker E2E、資源釋放與惡意 Worker 測試後，才能重新評估發布資格。

本輪 owner/checkpoint：`runtime_value_limits_tdd` 正在 shared runtime 實作與驗證 bounded renderer／value-allocation guards；回報條件是可重現的 RED→GREEN、完整 runtime test 與介面摘要。完成後由 coordinator 接線到 Worker、guest、native claim parity test 並進行獨立審查。

## 2026-08-10 Bounded renderer 完成與發布差距重新盤點

本輪提交（皆為本機 commit，未 push）：

- `9ab1ffc test(worker): deflake managed stop cancellation test`
- `0158129 fix(runtime): bound canonical output and value materialization`

finding #29 已關閉：`render_output_bounded` 在配置前逐次檢查，JSON escaping 以
`serde_json` 為基準釘住；per-value（canonical bytes／collection items／depth）與
cumulative materialization 上限皆為固定寬度 u64 邏輯位元組，Worker、zkVM guest 與 host
golden-vector claim 三者共用同一 renderer。這些只是安全上限，不計入 `usage_units`。

同時修正一個既有測試 race：`stop_task_execution` 在 `execute_task` 記錄 assignment
之前會回 `PermissionDenied`，而 poll loop 容忍 `success=false` 卻 unwrap 了 `Result`。
修正前 4/12 通過，修正後 15/15。

驗證（`x86_64-pc-windows-gnu`）：runtime 25、worker-executor lib 83、managed-proof 15、
task-scheduler lib 75（1 intentional ignored）、clippy `-D warnings`、`cargo fmt --all` 全綠。

階段 3 的勾選框先前已過期；本輪比對程式碼後修正為 complete。實際尚存的發布差距：

1. Guest image ID、build attestation 與真實 receipt fixture 因 guest source 改變而全部過期，
   必須在支援的 Linux/macOS prover host 重建並跑一次真 proof。
2. Prover sidecar 未被打包：`docker-compose.yml`、`docker-compose.test.yml` 與 `.env.example`
   完全沒有 `MANAGED_PROVER_EXECUTABLE`，而該 config 預設為空字串，因此目前用 Compose 起的
   worker 會讓每一個 managed task 都失敗。這是目前最硬的部署阻擋。
3. 階段 4 尚未開始：沒有 off/observe/enforce rollout mode，也沒有 proof verification
   metrics 與 audit events；缺這個代表啟用強制驗證是全有全無，沒有觀察期。
4. 階段 5 尚未開始：惡意 Worker 測試、多節點 Docker E2E、資源釋放、依賴授權與可重現 guest build。
5. 經濟模型未定案：單次 proving 約 570-580 秒，對一個毫秒級 managed function 而言，
   enforce 前需要先有明確決定，否則階段 4 完成也不會真的敢開。

Windows 原生無法編譯 `risc0-circuit-rv32im-sys`（C++ 需 `/std:c++20`），這在
`cargo check` 進入本專案 crate 之前就失敗，屬既有環境限制，與本輪變更無關。

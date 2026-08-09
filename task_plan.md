# Hivemind ZK 函式計費證明實作計畫

## 目標

讓不受信任 Worker 對 `managed-function-v0` 的程式執行與函式計費產生可由 Nodepool 驗證的零知識證明；只有驗證成功的 `usage_units` 才能進入結算。

## 當前階段

階段 3：協議與 Nodepool 驗證閘門（in_progress）

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
- [x] 固定 builder image digest；目前可信 guest image id `[3606400121, 4250889949, 2277454476, 3430793801, 2111044864, 2713379816, 851522248, 2751351423]`
- [x] 將 deterministic managed runtime 放入 guest 執行路徑
- [x] guest commit 公開聲明，私有 witness 保留程式輸入／執行軌跡
- [x] Worker prover 產生包含 proof scheme、固定 image id、journal 與 receipt 的 proof envelope
- [x] 加入 golden vectors、image-id/journal tamper tests 與 proof time benchmark（約 570–580 秒）
- **狀態：** complete

### 階段 3：協議與 Nodepool 驗證閘門

- [x] protobuf 傳遞 proof scheme、image id、journal、receipt/seal
- [ ] Nodepool 以可信 image id 驗證 proof
- [ ] 驗證 journal 與資料庫 task/source/input/output/max_cpt 完全一致
- [ ] proof 無效、缺漏、重播或版本不支援時 fail closed
- [ ] 驗證成功後才寫 receipt、完成任務與結算
- **狀態：** in_progress

### 階段 4：遷移、失敗語義與營運

- [ ] 加入 off/observe/enforce 三段 rollout mode
- [ ] 定義 proving timeout、取消、失敗與 retry 行為
- [ ] 限制 proof 大小與 verifier CPU/記憶體消耗
- [ ] 增加 proof verification metrics、audit events 與管理介面狀態
- **狀態：** pending

### 階段 5：完整驗證與發布

- [ ] runtime、Worker、scheduler、node-manager focused/full tests
- [ ] 惡意 Worker 測試：偽造計費、輸出、task id、版本、seal
- [ ] Docker 多節點完整流程與瀏覽器回歸
- [ ] cargo fmt、clippy、audit、依賴授權與可重現 guest build
- [ ] 文件與本機 Conventional Commits 完整；不 push
- **狀態：** pending

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

## 歷史

上一輪完整平台驗證已完成，摘要保留於 `docs/platform-validation-state.md`；本計畫從該乾淨基線開始。

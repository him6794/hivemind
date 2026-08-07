# Hivemind ZK 函式計費證明實作計畫

## 目標

讓不受信任 Worker 對 `managed-function-v0` 的程式執行與函式計費產生可由 Nodepool 驗證的零知識證明；只有驗證成功的 `usage_units` 才能進入結算。

## 當前階段

階段 2：zkVM guest 與 proof 產生（in_progress）

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
- [ ] 固定 builder image digest、guest image id 與供應鏈驗證方式
- [ ] 將 deterministic managed runtime 放入 guest 執行路徑
- [ ] guest commit 公開聲明，私有 witness 保留程式輸入／執行軌跡
- [ ] Worker prover 產生 proof envelope
- [ ] 加入 golden vectors、tamper tests 與 proof size/time benchmark
- **狀態：** in_progress

### 階段 3：協議與 Nodepool 驗證閘門

- [ ] protobuf 傳遞 proof scheme、image id、journal、seal
- [ ] Nodepool 以可信 image id 驗證 proof
- [ ] 驗證 journal 與資料庫 task/source/input/output/max_cpt 完全一致
- [ ] proof 無效、缺漏、重播或版本不支援時 fail closed
- [ ] 驗證成功後才寫 receipt、完成任務與結算
- **狀態：** pending

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
| PowerShell glob 直接傳給 `rg` 造成 Windows path error | 1 | 改用 `Get-ChildItem` 展開檔案；不影響程式碼 |
| workflow-orchestrator 引用的 `skills/auto-trigger/SKILL.md` 不存在 | 1 | 記錄為工具環境限制，不阻塞實作 |
| 首次持久化補丁因 findings 檔頭比對失敗 | 1 | 拆分為小補丁，避免部分更新 |
| MSVC workspace test 無法連結 MinGW `libtailscale.a` | 1 | 依既有平台契約改用 `x86_64-pc-windows-gnu`，246 tests 通過 |
| `postcard`／`bincode` 引入未維護依賴警告 | 2 | 改用 workspace 既有 `serde_json`，新警告歸零 |

## 歷史

上一輪完整平台驗證已完成，摘要保留於 `docs/platform-validation-state.md`；本計畫從該乾淨基線開始。

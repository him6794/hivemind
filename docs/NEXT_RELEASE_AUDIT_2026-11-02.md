# 功能驗證與下一版建議

日期：2026-11-02  
對象：目前工作區（HEAD `1755e64a` 加上既有未提交修改），不是只測已發布的 0.1.0 安裝檔。

## 修復結果（2026-11-03）

下方「結論」與「發布前應修復的項目」是 2026-11-02 修復前的紀錄，保留作為對照。以下為修復後的狀態。

| 項目 | 修復內容 | 驗證結果 |
|---|---|---|
| 改密碼後舊會話 | 改密碼時，同一交易撤銷其他會話並保留發起改密碼的目前會話；重設密碼撤銷全部會話；沒有會話 ID 的舊權杖改以 `users.password_changed_at` 判斷，改密碼後即失效 | 新增整合測試 `changing_or_resetting_the_password_ends_the_other_sign_ins`；真實服務端到端 **25/25**（修復前 23/25） |
| 登入與改密碼的競態 | 登入在驗證舊密碼到寫入會話之間，對使用者列加共享鎖（`FOR SHARE`）；改密碼取得排他鎖，兩者不能交錯 | 由程式推理與 Codex 獨立審查確認；未加入並行壓力測試 |
| 權杖截止時間的時鐘 | `password_changed_at` 改用應用程式時鐘寫入，與權杖 `iat` 同一來源，避免資料庫與應用程式時鐘偏差 | 同上；auth、database、node-manager 測試全數通過 |
| 靜態匯出的 UI 測試 | Playwright 網站服務改用 `frontend/scripts/serve-static-export.mjs`，行為與 Cloudflare Pages 相同（乾淨網址、`404.html`、`/api` 代理未設定時回 503） | 原始 `npm run test:ui` 直接執行：**39/39**；帳戶瀏覽器流程 9/9；伺服器單元測試 8/8 |
| 發布建置與煙霧測試 | `build-release-frontends.ps1`、`release-frontend-smoke.ps1` 改檢查 `out/index.html`；官網不再要求 `WEBSITE_NODEPOOL_GRPC_ADDR`（靜態站不讀執行期變數） | 三個正式前端建置成功；發布相關契約腳本 11 個全部退出碼 0 |
| Docker Compose 站台 | `frontend/Dockerfile` 已於 `766fae54` 刪除，但 compose 的 `site` 仍指向它；新增多階段 Dockerfile（建置後以靜態伺服器提供 `out/`），移除已不存在的 Next.js 伺服器需求 | `docker compose config` 通過；`hivemind-site` 映像建置成功；容器實測：`/` 與 `/docs/loops` 200、`/api/account` 503、未知路徑 404 |
| Rust 格式與 lint | `docs_examples.rs` 的 import 排序，以及 56 處不必要的 raw string 井號（只改語法標記，字串內容不變，已逐行核對） | `cargo fmt --check` 通過；執行器與 hivemind-rs 的嚴格 clippy（`-D warnings`）通過 |
| 安裝說明 | 網站步驟改為「執行安裝程式、只安裝到目前 Windows 帳戶、從開始功能表開啟」，移除過時的「解壓縮」；進階設定改為指向安裝資料夾 | 依 `scripts/installer/hivemind-app.nsi` 與 Worker 打包腳本核對；`npm test` 97/97 |

最終一輪測試數字：

- hivemind-rs 完整工作區（CI 指令 `cargo test --workspace --all-targets --locked`）：859 通過、0 失敗、1 忽略。忽略的是需要外部網路的 Resend 測試。
- 執行器工作區：317 通過、0 失敗。
- 真實服務端到端（一個 Nodepool、一個 Master、三個原生 Worker、BFF）：25/25。
- 官網前端單元與契約測試：97/97。
- 瀏覽器 UI（原始 `npm run test:ui`）：39/39。

仍然未完成或有風險的項目：

- **安裝程式仍未簽章。** 需要有效的程式碼簽章憑證，本次沒有以自簽憑證代替。
- **改密碼保留的是發起改密碼的那個會話，這是刻意的設計。** 改密碼本身需要舊密碼；若攻擊者同時持有該權杖與舊密碼，他本來就能重新登入，因此這不是新的繞過。其他會話與所有舊權杖都會失效。
- **登入競態修正沒有並行測試**，目前只有程式推理、既有整合測試與 Codex 審查。
- **未驗證：** 所有服務的完整 `docker compose build`（只建置了 `site`）、`release-stack-smoke` 全棧執行（只跑了契約測試）、真實郵件投遞、正式 Turnstile、正式部署。
- **未處理：** 公開 BFF 主機的 `/health` 回 404，已確認是公開路徑未轉送，不是服務故障，未做變更。
- 本次變更尚未 commit，依指示未提交。

## 下一版更新（2026-11-03）

本次針對「用起來順不順」與「能不能處理一批真實輸入」做了四項更新，每項都經過真實服務驗證：

| 更新 | 之前的問題 | 現在 | 驗證 |
|---|---|---|---|
| 失敗原因 | 所有失敗都只顯示「quorum 未達成」，分不出是程式錯誤、預算不足或其他原因 | 副本一致回報同一代碼時，訊息會帶上代碼 | 真實服務：除以零的任務顯示 `managed consensus quorum was not reached: replicas failed with runtime_error`；預算不足的任務顯示 `…budget_exhausted`；調度器單元測試 `no_quorum_reason_names_a_failure_only_when…` |
| 結果輸出 | `hivemind result` 對已完成的 hcode 任務只顯示空白的 result torrent，看起來像失敗 | 顯示 `Output:`，內容來自任務列表的 `output` 欄位 | CLI 單元測試 `result_shows_the_output_of_a_managed_task`；端到端 `CLI result prints the output of a completed managed task` |
| 批次執行 | 多筆輸入只能逐筆 `submit`，沒有總預算上限、速率控制與結果彙整 | `hivemind-bin batch <source> --inputs <jsonl> --max-total-cpt <n>`：先報價，「任務數 × 每筆上限」超過總上限就整批拒絕；預設每分鐘 50 筆，低於 Master 的 60 筆上限；遇到 429 等待視窗重置；輪詢到結束；寫出 CSV；只有全部 `COMPLETED` 才以成功結束 | 真實服務：三筆輸入得到 2、4、6，各計費 42 CPT；`batch runs the function over every input line…` |
| 密碼來源 | CLI 只能用 `--password`，密碼會出現在命令列與程序清單中 | 可用環境變數 `HIVEMIND_PASSWORD` 代替 | CLI 單元測試；端到端批次命令改用環境變數提供密碼 |

修復過程中也修正了兩個我自己引入的問題，都已由測試覆蓋：

- 批次的報價請求缺少正數的 `max_cpt`，被伺服器的 hcode 契約拒絕（400）。報價只計算資源，所以請求帶一個佔位值 1，單元測試 `batch_quote_carries_a_positive_budget…` 會擋住回歸。
- 第一版把每筆上限預設成報價，結果任務一開始就 `budget_exhausted`。報價不含程式執行的用量，所以預設改為 100 CPT，且不得低於報價。

刻意沒有改動的設計：`submit` 仍只接受 `--input <json>`，既有測試保證 `--input-file` 會被拒絕；多筆輸入改由新命令 `batch --inputs` 處理。

最終驗證（2026-11-03）：

- hivemind-rs 完整工作區（CI 指令）：866 通過、0 失敗、1 忽略（忽略的是需要外部網路的 Resend 測試）；較上一輪多出的 7 項為本次新增測試。
- 嚴格 clippy（`-D warnings`）：通過。
- 執行器工作區：317 通過、0 失敗。
- 真實服務端到端：28/28，包含批次命令與 `result` 輸出。

批次目前的限制：

- 命令中途中斷不會續跑，CSV 只在結束時寫出。
- 每筆上限預設 100 CPT；程式用量超過會以 `budget_exhausted` 失敗，需要用 `--max-cpt-per-task` 調高。
- 一次最多 1000 筆；`--max-total-cpt` 必填，不會替使用者決定預算。

尚未完成、依優先順序排列（未驗證，不列為已完成）：

1. Master 網頁的「第一個任務」導引，以及結果頁顯示失敗原因、輸出與計費。
2. 批次續跑：把批次狀態存在本機，中斷後可接續。
3. HTTP 版批次 API 與 Python SDK，供排程或整合使用。
4. 模板選單（例如 `batch --template sum`），降低撰寫 hcode 的門檻。
5. 試點：找 5 到 10 位有固定批次需求的人實際使用，量測第二次使用率。這是產品假設，程式測試無法替代。

## 結論

核心的 `hcode` 任務閉環已能在真實服務中完成：註冊與驗證信、取得歡迎 CPT、Master 提交、三個不同測試帳戶的 Worker 執行、形成 quorum、取得輸出、下載輸出、結算、取消與執行失敗不收費。

但目前不能標為「全部功能通過」，也不建議直接擴大導流：

1. **改密碼與忘記密碼重設都沒有撤銷舊登入會話。** 已用真實 Nodepool 重現，應先修復。
2. **官網靜態匯出與測試／發布腳本仍有不一致。** 網站能建置，但原始瀏覽器啟動命令及共用發布產物檢查會失敗。
3. **安裝入口有可修正的摩擦。** 正式下載可用、雜湊相符，但安裝程式沒有 Authenticode 簽章，說明仍是 ZIP 解壓縮流程。
4. **已有技術能力，不代表已證明市場價值或效能優勢。** 下一版應把既有能力組成可重複使用的批次工作流程，而非再增加一批零散功能。

建議下一版主題：**讓使用者不必先學 hcode，也能完成第一批有用的工作，並願意回來使用。**

## 1. 實際測試結果

### 自動測試與建置

| 範圍 | 結果 | 說明 |
|---|---|---|
| 主 Rust workspace | 858 通過、0 失敗、1 ignored | 真實獨立 PostgreSQL；強制資料庫不可略過 |
| executor workspace | 317 通過、0 失敗 | Windows、預設 features；Rust 1.98.1 與 CI 指定的 1.90.0 各重跑一次，均通過；不代表 Linux OCI／CUDA 實機驗證 |
| Windows Master／Worker WebView helper | 36 通過 | 兩個原生 helper 各 18；不是完整 GUI 安裝驗收 |
| Rust 文件測試 | 2 通過 | 兩個 workspace 各 1 |
| 官網單元／契約 | 89 通過 | 包括註冊步驟、hash 路由、帳戶政策 |
| BFF 單元測試 | 51 通過 | CAPTCHA、session cookie、輸入及權限錯誤等 |
| Master UI 單元測試 | 68 通過 | 提交、任務、session、語系及 UI 政策 |
| Worker UI 單元測試 | 66 通過 | 登入、容量、狀態與 UI 政策 |
| 桌面與 native bridge 契約 | 27 通過 | 10 + 17 |
| 原有 Playwright UI 測試 | 39 通過 | 改用適合 static export 的本機伺服器後；API 為模擬 |
| 補充帳戶瀏覽器流程 | 9 通過 | 找回密碼／帳號、驗證成功／過期、重設、未驗證轉帳限制、轉帳重試、改密碼；API／CAPTCHA 為模擬 |
| 三個前端正式建置 | 3/3 成功 | 官網、Master、Worker |
| 安裝／Compose／發布腳本契約 | 9 組通過 | 靜態契約／CheckOnly 不等於真正安裝或容器隔離驗收 |
| 主 Rust 嚴格 Clippy | 通過 | `--workspace --all-targets -- -D warnings` |
| executor 嚴格 Clippy | 失敗 | `docs_examples.rs` 的 56 個 `needless_raw_string_hashes` 診斷；1.98.1 與 CI 的 1.90.0 均重現 |
| Rust 格式檢查 | 失敗 | `docs_examples.rs:8` 的 import 排序；兩個 workspace 檢查均遇到此檔 |
| 原始 `npm run test:ui` 啟動 | 失敗 | `next start` 不支援 `output: export` |
| 共用前端發布產物檢查 | 失敗 | 仍尋找 `.next/standalone/server.js`，而非 `out/` |

不重複計算重跑結果：單元、整合、原生 helper 與文件測試共 **1,514 項通過**；瀏覽器互動另有 **48 項通過**。腳本「組數」及以下真實服務情境沒有混入這個數字。

唯一 ignored 的主 workspace 測試為 `account::tests::resend_api_answers_over_tls_for_an_invalid_key`，需要外部 Resend 網路存取；此次沒有把它算成通過。

### 真實服務端到端

最終執行結果：**23 項通過、2 項失敗**。不是 mock gRPC，也不是只呼叫內部函式。

環境：

- 本次原始碼編譯的 `hivemind-bin.exe`。
- 一個 Nodepool、一個 Master、三個原生 Windows Worker、一個 Node.js BFF。
- 三個不同測試 provider 帳戶；同一台主機，不代表三個獨立營運者。
- 專用 Docker PostgreSQL／Redis；每輪另建端到端資料庫，未碰正式資料庫。
- 驗證與重設郵件採 Nodepool 支援的本機 file transport，不寄外部郵件。
- 本機停用網站 VPN enrollment；沒有冒充公網 VPN 驗收。

通過的實際行為：

- 註冊、登入、讀取本機驗證信、驗證後發放 1,000 CPT。
- 驗證 token 不能重複領取歡迎金；重複帳號與錯誤密碼遭拒。
- 相同 idempotency key 的 CPT 轉帳只扣款一次。
- 改密碼後舊密碼無法重新登入；重設 token 僅能用一次。
- 找回帳號對存在／不存在的信箱回傳相同公開訊息。
- 登出後該 session 立即失效。
- BFF 拒絕沒有 proxy secret、沒有 session 或缺 CAPTCHA 設定的請求；官網 BFF 不提供任務 API。
- Master／Worker 真實 HTTP 入口與本機 UI 可用；未登入的操作遭拒。
- 三個 Worker 在不同 provider 帳戶下註冊。
- Master 提交 `get(input, "value") * 2` 類型的 hcode，輸入 21，得到 42；取得 quorum 後完成。
- 從 task log 讀取結果、透過 artifact endpoint 下載結果。
- 實際扣款等於 Nodepool 已結算費用，且未超過 `max_cpt`。最後一輪：輸出 42、3 個副本／quorum 2、扣款 42 CPT（上限 100），帳戶由 990 降至 948；約 587 ms 為本機單次煙霧測試觀察值，不是性能基準。
- 重複 task ID、跨帳戶取消及超出預算的資源報價遭拒。
- 未派送任務取消後為 `CANCELLED`，帳面 `billed_amount=0`；刻意提交的錯誤程式為 `FAILED`、`billing_settled=true`、0 CPT。
- CLI `status`／`result` 可連到真實 Master。

兩個失敗均屬同一類安全缺口：

1. 改密碼之後，另一個先前登入的 session 仍可 `GetAccount`。
2. 完成忘記密碼重設後，重設前的 session 仍可 `GetAccount`。

這不是要求每次改密碼都強制登出目前畫面；測試特別區分了**另一個登入會話**與**重設之前的會話**。

### 正式網站唯讀驗證

沒有在正式環境建立帳戶、寄信、提交任務、轉帳或更動部署。

- 首頁：HTTP 200。
- `/docs/loops`：HTTP 200；實際瀏覽器進入 `/#/docs/loops`，顯示正確文件。
- 未登入的 `/api/account`：HTTP 401。
- 下載頁的兩個 0.1.0 `.exe`：HTTP 200；實際下載至各自新建的暫存目錄，沒有執行安裝。
- Master 安裝檔 SHA-256：`935040b5b0a97e00aac105ac1b0f04c60969e4b8b08e93798b1256e82cacfbb8`，與官網相符。
- Worker 安裝檔 SHA-256：`de3d226d74b62a97dc691a3f55aa7449adc7669ad89b44141ee7eab9596a32e4`，與官網相符。
- 兩個檔案的 Authenticode 狀態均為 `NotSigned`。
- 公開 BFF hostname 的 `/health` 回傳 404；這只能證明該公開路徑不可用，不能據此認定帳戶服務故障。本機 BFF `/health` 已驗證成功。

## 2. 下一次發布前應修復的項目

### P0：密碼重設後撤銷舊會話

證據：

- `hivemind-rs/crates/node-manager/src/grpc.rs:1056`：改密碼只更新 password hash。
- `hivemind-rs/crates/node-manager/src/grpc.rs:1123`：重設密碼消耗一次性 token 並更新 hash，但沒有撤銷既有 sessions。
- `hivemind-rs/crates/auth/src/lib.rs:69`：驗證 session 檢查的是 JWT 與 session 是否仍 active，沒有與新密碼連動。
- `hivemind-rs/crates/auth/src/session_store.rs:31`：active 判定依賴 `revoked_at` 與到期時間。
- `test_logs/release-audit-20261102-afe46ed7/live-final/live-stack-results.json`：兩個真實 API 失敗情境。

影響：若登入 token 已被取得，使用者改密碼或利用信箱找回帳號，仍未必能中止先前的登入存取。

驗收：

- 忘記密碼重設，必須在更新密碼的同一交易中使所有舊使用者登入會話失效。
- 已登入的改密碼可以保留目前會話，但其他會話必須失效；或採全部登出的一致政策。
- 舊 token 不能讀取帳戶、轉帳、刷新或重新建立受保護操作；新密碼登入正常。
- 明確處理沒有 session ID 的舊 token 相容策略，不能只修新會話資料表而留下舊 token 路徑。
- 不混淆使用者登入 token 與任務綁定的 Worker 執行憑證。

### P0：對齊 static export 的發布與 CI

證據：

- `frontend/next.config.ts:24`：`output: "export"`。
- `frontend/playwright.ui.config.mjs:46`：仍以 `next start` 啟動。
- `scripts/build-release-frontends.ps1:12`：仍要求 `.next/standalone/server.js`。
- `.github/workflows/ci.yml:186`：官網測試入口沒有另外執行新加入的 BFF 測試。
- `executor-rs/crates/managed-function-runtime/tests/docs_examples.rs:8`：格式及嚴格 lint 問題。

驗收：乾淨 checkout 可產出官網 `out/` 與兩個本機 UI；CI 用與部署相符的靜態路由跑瀏覽器測試；BFF 測試納入必要門檻；格式與 strict Clippy 全綠；不能靠預先存在的舊 standalone 產物過關。

目前 39 項 UI 全過是**修正測試環境後**的結果，不表示原始測試啟動設定已修復。

### P1：把下載到第一次成功之間的斷點修順

已驗證的問題：正式網站提供 NSIS `.exe` 安裝程式，步驟卻仍寫解壓縮。相同文案位於 `frontend/src/lib/hivemind-site-data.mjs:423`、`:430`、`:1138`、`:1145`。

建議：

- 依安裝檔／portable ZIP 明確分開步驟，移除過時解壓縮說明。
- 建立 Windows code-signing 發布流程；簽章本身不保證 SmartScreen 立刻沒有警告，但應避免長期要求新使用者自行判斷未知發行者。
- 發布版本與 Nodepool 協定相容性做啟動前檢查，顯示可操作的錯誤及更新方式。
- 以乾淨 Windows 使用者環境驗證安裝、首次登入、重啟、升級及解除安裝；現有 helper 單元測試不替代這些驗收。

## 3. 下一版產品範圍

不要再把已經存在的註冊、找回密碼、1,000 CPT 歡迎金、文件範例、預算限制、取消任務或三副本共識列成新功能。下一版要補的是它們之間的使用流程。

| 優先順序 | 建議交付 | 為何有助用戶與價值 | 最小驗收 |
|---|---|---|---|
| 1 | Master 內的「完成第一批工作」導引 | 降低先學語法、貼程式碼、理解低階資源欄位的成本 | 將既有模板接成 2–3 個有輸入／輸出規格的表單流程；匯入自己資料、本機預檢、顯示上限、提交、匯出結果 |
| 2 | Python SDK + batch/job 概念 | 把一次性的 UI 示範變成可整合、可重複執行的工作 | 經本機 Master API 提交；分片、進度、取消、部分重試、結果合併、idempotency、整批預算上限 |
| 3 | 提交前的可執行性與費用說明 | 避免第一個任務長期 Pending、預算被拒卻不知怎麼改 | Nodepool 回傳符合需求的容量、不同 provider 帳戶數、缺少條件與可行動原因；保留原 quorum，絕不默默降級 |
| 4 | 可匯出且可對帳的工作結果 | 使用者需要帶走成果，不只是看狀態變成完成 | batch 結果 ZIP／JSON／CSV、每個項目狀態、輸入／模板版本、已結算 CPT、失敗原因；清楚標示資料是否部分完成 |
| 5 | 小規模需求端試點與漏斗量測 | 先證明有人重複使用，再擴大招募 Worker 或廣告 | 找到 5–10 位有固定批次工作的人，觀察其第二次實際使用；量測阻礙，而不是只計帳號或節點數 |

### 建議先驗證的應用假設

這些是產品假設，不是已取得市場需求的結論。先找 5–10 位設計夥伴帶實際資料與目前做法來驗證，選出一個值得投入的場景，再決定模板與 SDK 的細節；不要等整版做完才找人，也不要同時開發三個垂直市場：

1. **教學／研究用的公開資料參數掃描。** 同一個純函式套到多組輸入，以可重現的設定取得結果。適合測試批次提交與合併；先證明執行時間足以攤平網路及副本成本。
2. **規則／設定的批次回歸驗證。** 對公開、合成或去識別化資料執行固定規則，比對不同規則版本的結果。價值應是可重現、自動化、結果對照，而不是宣稱簡單資料處理必然比本機快。
3. **受控 Linux provider 的既定批次工作試點。** 若需求明確落在 ONNX／CPU 資料處理等一般計算，可沿既有 OCI 邊界驗證一個固定工作負載；沒有真實沙箱、相容性、成果及成本數據前，不應擴張成「任意 GPU／LLM 雲」。

對前兩項，先用既有 hcode 可表達的工作做訪談與原型。若使用者實際需要 Python 生態、檔案、網路或大型資料，應明說 closed DSL 不適合，不能靠增加更多小範例掩飾缺口。

公網 Worker 不保證輸入保密。不要把私人財務資料、個資或商業機密作為預設導入場景。

### 必須先做的效益比較

對每個候選場景，使用相同輸入與可比較的結果規格，測量：

- 本機執行 vs. Hivemind 的完成時間、使用者等待時間。
- 提交、排隊、傳輸、計算、quorum、結算各階段時間。
- 一整批的實際 CPT、重試數、人工介入時間。
- 小量／中量／大量輸入是否真的有分散執行收益。

本次單一小算式煙霧測試不構成吞吐量、SLA 或對本機效能優勢的證據。不要從功能測試通過直接推導「更快、更便宜」。

## 4. 成效指標與發布門檻

以下是建議驗收目標，**不是目前已量測的產品數據或增長承諾**。

- **核心指標：**每週再次提交自己實際工作、並成功取得結果的需求端使用者數。
- **導入漏斗：**到站 → 註冊 → 信箱驗證 → 安裝啟動 → Master 登入 → 首次成功 → 首次自己的資料 → 7 日內再使用。
- **第一次體驗目標：**預先有可用容量的試點中，從安裝完成至模板首個成功結果，中位數不超過 5 分鐘。
- **可用性：**所有等待／拒絕狀態都有原因；未達 quorum 不當作成功；失敗及取消不誤收費。
- **留存驗證：**先對 5–10 位設計夥伴觀察兩週，不以廣告註冊數或自建 Worker 數判定產品成立。
- **量測邊界：**不記錄密碼、token、完整使用者輸入或程式碼；需求端與供給端分開計算，測試帳戶和示範任務排除。

建議交付順序：

1. 修會話撤銷、static export 發布路徑、lint 與安裝文案。
2. 選一個具體工作流程，完成 Master 模板導引及可攜結果。
3. 補 SDK／batch，讓同一位使用者能反覆使用自己的資料。
4. 以真實使用及與本機的對照數據，決定是否投資 Linux OCI 工作負載或擴大供給。

## 5. 不建議放入這一版的內容

- 再做一次純視覺改版；目前畫面不是唯一或最有證據的主要阻礙。
- 還沒驗證需求就廣泛招募 Worker、增加推薦獎勵或承諾收益。
- 任意 Python／Docker／GPU／LLM 工作負載全面開放。
- 把任務提交或 Worker 控制搬到官方網站。官網仍只負責介紹、帳戶與下載；SDK 及工作導引使用本機 Master。
- 為了讓首個任務完成而降低 quorum、忽略 owner 規則、信任 Worker 自報計費，或把 replica agreement 宣稱為獨立正確性證明。

## 6. 覆蓋限制

以下沒有完成驗收，不能從此次綠色測試推論可用：

- 真實 Resend 郵件投遞、收件匣到達率及正式 Turnstile 驗證。
- 正式網站註冊到正式 Master／Worker 的完整跨裝置流程。
- 公網 Headscale enrollment、NAT、實際斷線重連、跨主機及跨版本互通。
- Linux rootless OCI 的真實隔離 E2E、CUDA／ONNX 硬體 E2E、ARM64／macOS。
- 安裝程式實際執行、升級／回滾、Windows 信任提示及 GUI 全生命週期。
- 完整 Docker Compose 映像重建與 release stack E2E；本次只有腳本／配置契約，且共用前端發布產物檢查已明確失敗。
- 長時間壓力、故障恢復、跨機共謀、可用性 SLA、真實成本效益與使用者留存。
- 官方 0.1.0 安裝檔與目前工作區的功能一致性；這次只驗證了它們可下載、雜湊相符及簽章狀態。

## 7. 證據與重現

本次完整日誌、JSON、畫面截圖：

`test_logs/release-audit-20261102-afe46ed7/`

重要檔案：

- `test-counts.json`：各組測試數字，不把重跑次數加成覆蓋量。
- `rust-workspace.log`、`executor-workspace.log`、`native-webview-tests.log`。
- `browser-clean-urls/results.json`、`account-browser-verified/results.json`。
- `live-final/live-stack-results.json`：最後一輪真實服務情境。
- `public-browser-results.json`、`installer-verification.json`。
- `browser-default.log`、`release-frontend-artifact-check.log`、`executor-clippy.log`：原始失敗證據。

臨時測試程式在 `tmp/release-audit-20261102-afe46ed7/`。未修改業務邏輯，沒有 commit、push 或部署；前端建置會更新 Next.js 生成檔與建置產物。本次啟動的服務均已停止，專用 PostgreSQL／Redis 容器及其暫存 volume 已移除；保留報告、日誌與截圖。

重現資料庫測試時必須使用隔離資料庫，且設定 `HIVEMIND_REQUIRE_TEST_DATABASE=1`。本次使用 `-j 1` 避免記憶體壓力；Rust 的 all-targets 測試與 doc tests 分開執行。

測試過程修正了三類測試環境假設，沒有把它們列為產品 defect：普通靜態伺服器不會解析 Next export 的 clean `.html` URL；測試帳號不能使用預先指定的 admin 名稱；Worker 的 bind 必須與 advertise 可達性一致。高 CPU 需求的取消 fixture 也必須先通過報價上限檢查。

## 8. 外部第二意見與調研狀態

本次另行安排 Codex 第二意見與 agy 官方資料查證。窄範圍 Codex 安全審查呼叫已逾時。官網側 Codex 回應則受到 Windows read-only sandbox 阻擋，沒有成功讀取專案檔案，因此不當作有效的獨立程式審查。另一路 runtime 第二意見在測試結束時仍未完成，已停止，沒有列入評估依據。

委派回應曾把 `downloads.mjs` 的空白預設值推論成「正式網站沒有下載版」。這項結論不成立，未採納：`downloadFor` 會讀取建置時的 `NEXT_PUBLIC_RELEASE_DOWNLOADS`，而正式網站兩個安裝檔已實際下載並驗證雜湊。已實作的分步註冊與帳戶恢復也沒有重列成待開發功能。

agy 回傳 BOINC、Modal、Ray 的研究；Golem 呼叫逾時，未列入證據。主程序再次擷取以下官方頁面，僅採用能直接核對的內容（2026-11-02）：

- [Modal 入門](https://modal.com/docs/guide)：明確提供 `pip install modal`、`modal setup`、最小程式與 `modal run`。採用的啟示是連續、可操作的首次執行路徑；不引用沒有在此頁核對到的上手分鐘數、串流輸出或用戶規模。
- [Ray Jobs Python SDK](https://docs.ray.io/en/latest/cluster/running-applications/job-submission/sdk.html)：直接展示 `submit_job`、`get_job_status`、`get_job_logs`、`stop_job`。採用的啟示是把完整任務生命週期提供給使用者程式，而非只有提交 API。
- [BOINC Validation wiki](https://github.com/BOINC/boinc/wiki/Validation)：再次擷取時只取得 wiki 首頁及載入錯誤，未能復核研究回傳的驗證細節，因此不以該頁的 quorum 敘述作本報告依據。

不採用外部研究中「信任機制已完全解決」或「3 台獨立機器全部一致即保證正確」的推論。Hivemind 的認證 Worker 一致性不是獨立正確性證明；同帳戶／不同帳戶與實際營運者獨立性也不是同一件事。需求假設仍須由實際使用者和效益比較驗證，不能由競品文件推導成長百分比。

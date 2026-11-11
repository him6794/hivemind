# 第二階段：自動化、經濟與語言（2026-11-11）

前置：`NEXT_VERSION_PLAN_2026-11-10.md` 的「本版不做」五組項目。這裡逐項記錄範圍、決策與驗收；需要使用者或外部資源才能完成的部分列在「阻塞」，不以假完成處理。

## 1. 伺服器端 Job、API token、Python SDK

### 憑證模型（Nodepool 是唯一權威）

- Token 格式：`hmk_` 加 64 個十六進位字元（32 位元組，來自作業系統亂數）。明文只在建立時回傳一次。
- 儲存：只存 SHA-256 雜湊、名稱、前綴（顯示用）、建立時間、到期時間、最後使用時間、撤銷時間。Master 與資料庫都不存明文。
- 到期：預設 90 天，可指定 1 到 365 天。每位使用者最多 20 個有效 token。
- 建立：只能用登入會話建立；用 API token 不能再建立 token，避免權限自我擴散。
- 撤銷：使用者可撤銷單一 token；改密碼與重設密碼時，與登入會話一起撤銷全部 token（沿用既有的 `end_account_sessions` 交易）。
- 驗證：Nodepool 的 `validate_session` 認得 `hmk_` 前綴並查雜湊；Master 只轉送不透明的 bearer，不做自己的判斷。

### Job

- Job 由 Nodepool 持有狀態：一組相同來源的 hcode 函式，每個輸入是一個任務（`job-<id>-<序號>`）。
- 預算：每筆有自己的上限；整個 Job 有總上限。Nodepool 在提交前檢查「已完成花費 + 已保留上限 + 本筆上限」不超過總上限，超過的輸入標為 `refused`，不送出。
- 提交：背景 runner 每幾秒提交一批待送輸入，並把已送出的任務狀態同步回 Job。
- 取消：待送的輸入直接標為 `cancelled`；已送出的任務走既有的停止路徑。
- 結果：每筆輸出、花費、狀態可查；只有擁有者能看自己的 Job。
- 限制：每個 Job 最多 1000 筆輸入，單筆 JSON 輸入不超過 1 MiB，總輸入不超過 16 MiB，來源不超過 64 KiB。

### SDK

- Python 3 標準函式庫，沒有外部相依套件。
- 介面：`Client(base_url, token)`，提供 `quote`、`dry_run`、`submit`、`create_job`、`get_job`、`list_items`、`cancel_job`、`create_token`、`list_tokens`、`revoke_token`，以及 `wait_for_job`。
- Token 可由參數傳入，或讀取 `HIVEMIND_TOKEN`。

### 驗收

- DB 測試：token 建立、雜湊驗證、到期、撤銷、改密碼後失效、API token 不能建立 token。
- DB 測試：Job 建立、runner 提交、總上限拒絕、取消、擁有者隔離、結果查詢。
- Master：`hmk_` token 能通過中介層，不解析 JWT；路由轉送正確。
- SDK：以本機 HTTP 假伺服器測試每個方法的請求格式與錯誤處理。

## 2. 向量化計價與定期免費額度

- 向量化計價（V2 成本模型）：維持 `MANAGED_RUNTIME_V2_PLAN.md` 的 Gate 1（成本模型識別）與 Gate 2（真實買家）。兩個閘門都是文件中的決定，本階段不啟用，也不改現行計價。
- 定期免費額度：實作為 Nodepool 端的設定，預設關閉（`ACCOUNT_DAILY_ALLOWANCE_CPT=0`）。開啟時每位帳號每 24 小時最多領取一次，並受系統池餘額下限保護。金額與系統池預算是營運決定，由部署者設定。

## 3. 預設副本數

- 維持 3 個副本，不改預設。理由：`MANAGED_RUNTIME_EVOLUTION_PLAN.md` 在 2026-10-26 已決議保留 eager 預設；預設降為 2 時 quorum 仍是 2，等於要求兩個副本完全一致；任一副本失敗或不一致就不會形成證書，與 AGENT.md 中「不降低 quorum」的信任規則精神相抵觸。
- 使用者可以逐次指定 `--replicas 2` 或 `managed_replica_count`（第二階段之前已完成）。

## 4. Nodepool 抽樣重算與驗證

- 本階段實作純函式部分：依任務 ID 與抽樣率做可重現的抽樣決策，以及把驗證結果分類為 `verified`、`mismatch`、`unverified`。
- 不接入派工。接入需要兩個決定：驗證節點放在哪裡（不放在 Orange Pi，不放在 Master 或 Worker 的使用者機器），以及驗證算力的付款規則（由系統池支付或不付）。
- 驗證結果不改變既有結算；只有 `verified` 才能顯示為已驗證。

## 5. hcode 語言與發布

- 語言擴充依 `hcode-language-extension-2026-11` 的就地更新方針實作，每項功能分開提交：註解、`while`／`break`／`continue`、清單排序與附加、字典鍵、字串切割與連接。每項都更新語意清單摘要與文件。
- 阻塞：
  - 程式碼簽章：需要使用者提供簽章憑證。
  - 啟用更新通道：需要更新清單的託管位置與金鑰管理。
  - Linux headless Worker：需要 Linux 實機端到端環境（含 Headscale overlay）。
  - P2P 傳輸接線：需要第二台主機做跨主機驗證。

## 阻塞一覽

| 項目 | 缺少什麼 |
|---|---|
| 向量化計價 | 真實買家（Gate 2）與成本模型識別決定（Gate 1） |
| 免費額度金額 | 營運者決定金額與系統池預算 |
| 預設副本數 2 | 使用者明確同意收費與冗餘度變更 |
| 抽樣驗證接入派工 | 驗證節點位置與付款規則 |
| 程式碼簽章 | 簽章憑證 |
| 更新通道 | 託管位置與金鑰 |
| Linux Worker | Linux 實機端到端環境 |
| P2P 接線 | 第二台主機 |

## 完成狀態與驗證（2026-11-11）

本階段的提交都在本機，尚未推送，也沒有部署。

| 項目 | 狀態 | 提交（由舊到新） |
|---|---|---|
| 1. Job、API token、Python SDK | 完成 | `ec056304`（token）、`362b1541`（Nodepool job）、`aa162bc3`（clippy 修正）、`e213e06d`（Master 路由）、`12076c5d`（SDK）、`7e5314c7`（文件）、`471a3303`（site 測試） |
| 2. 定期免費額度 | 完成，預設關閉；向量化計價擱置 | `acfc10e2`、`a9d567a1` |
| 3. 預設副本數 | 維持 3，不改預設 | 無程式變更 |
| 4. 抽樣重算 | 純函式完成；未接入派工 | `de381757` |
| 5. hcode 語言 | 五項各自提交：註解、while／break／continue／if、append／sort、keys、split／join | `0bb62056`、`b238a618`、`1b1482db`、`2bed6de7`、`d47a18d0`；文件 `a51204ed` |

### 驗證（最終樹）

- hivemind-rs：`cargo fmt --all -- --check` 通過；`hivemind-bin` check 通過；node-manager 192 通過（連接 DB、序列執行）；master-api 71 通過；worker-executor 190 通過。
- 受影響 crate 的 clippy `-D warnings` 在各自提交時通過（database、node-manager、master-api、hivemind-bin、managed-consensus、managed-function-runtime）。
- executor-rs：managed-function-runtime 與 general-compute-runtime 的全部測試通過，每個語言提交都重新執行。
- 網站 104 通過；master-ui 98 通過；Python SDK 15 通過。

### 設計決定

- 為了讓 `break`、`continue` 能夠條件式使用，一併加入語句形式的 `if`。語句開頭的 `if` 先以語句讀取，失敗時回退為既有的運算式讀取，因此既有程式的含義不變。
- `while`、`break`、`continue`、註解與新 builtin 只在 CPU hcode 生效；frozen v0 不變。新 builtin 名稱加入保留名稱，使用者的 `fn` 不能使用同名（repo 內沒有此類程式）。
- Job：每個帳號同時最多 10 個進行中的 Job。原計畫未列，已寫入手冊。
- SDK 方法名稱為 `iter_job_items`、`jobs`、`task_log`，草稿中的 `list_items` 改名。
- 免費額度：API token 可以領取；所有拒絕都回 409，原因放在 `message`。
- 抽樣：`classify` 與憑證中的 `output_digest` 比對；只有 `Verified` 可以顯示為已驗證。
- 語意摘要（v1 digest）每個語言提交都會改變。目前為 `f7593109a627…`，同步寫在八個檔案與 master console 的 12 字元前綴。Worker 與 Nodepool 必須同版建置與升級。

### 審查

`codex_status` 顯示 Codex 未登入。依規則不代為登入，改為自我審查；獨立審查尚未進行，需要使用者登入 Codex 後再補。

### 未完成或擱置

- 向量化計價：Gate 1、Gate 2 仍需決定，未實作。
- 免費額度金額與系統池預算：由營運者決定，預設 0（關閉）。
- 抽樣接入派工：驗證節點位置與付款規則尚未決定。
- 程式碼簽章與更新通道：缺簽章憑證與託管位置。
- Linux headless Worker 的端到端驗證：需要 Linux 實機與 Headscale overlay，且要把新版 Nodepool 部署到 Orange Pi、讓 Linux Worker 加入 overlay。部署是對外動作，未經使用者確認不做。
- P2P 跨主機接線：需要第二台主機。

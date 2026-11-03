# hcode 分散式運算實作規劃

## 目標與邊界

- 保留 `hcode` 名稱與既有任務入口，不另立 hcode v2。語言語意變更仍須同步 semantics manifest、hash pins 與相容性測試。
- 大檔案透過具授權的 P2P 資料平面傳輸。Nodepool 管理身分、immutable manifests、來源登錄、租約、排程、預算、結果驗證與結算，不作為大檔案的常態中繼或內容資料庫。
- 優先重用現有 peer、CAS、overlay 與 transfer lease 協定。傳輸失敗不得以公開下載網址、弱化授權或無上限的 Nodepool 轉送繞過邊界。
- Master 與 Worker 都是不可信的參與者。來源、快取、硬體、使用量與結果宣告須由 Nodepool 依持久化任務及經驗證的證據判定；不同帳號或 IP 不等於真正獨立的提供者。
- 官方網站維持帳號與產品入口。任務提交和操作在 CLI／SDK／Master，Worker 控制維持本機介面。
- 封閉 hcode 不取得任意檔案、網路、程序或 native-library 存取能力。P2P、模型匯入與檔案輸出都由有界的宿主介面處理。
- 每個可獨立驗收的小功能完成測試後本機提交。既有帳號、網站與其他未提交工作不納入這批提交；不 push、不部署。

## 驗證原則

- 純函式單元測試、真資料庫授權／結算測試、真 RPC 傳輸及實際 CLI 流程分開記錄，不以 mock 測試取代完整流程。
- 資料庫測試必須設定 `HIVEMIND_REQUIRE_TEST_DATABASE=1` 並使用隔離資料庫。
- 跨 crate 改動檢查格式、角色隔離編譯及相關測試；階段完成時執行工作區回歸與嚴格 lint。
- 不以同機多個 Worker 的成功宣稱跨主機吞吐或真實故障域容錯。效能比較須記錄硬體、網路、驗證等級、有效結果數與實際成本。
- 重試、取消、到期、重複回報、舊 attempt、損壞內容和資源不足都必須有失敗路徑測試。不得重複結算同一 attempt。

## 功能清單

各項拆成可獨立提交的變更；未勾選項目不代表已實作。

### A. P2P artifacts 與重用

- [x] 盤點並確定既有 peer／lease／CAS 的接線契約與遷移方式。
- [x] 不讀取內容的 chunk 授權：共用當前副本檢查，供快取與 P2P 使用。
- [x] P2P 專用短效讀取權杖：用途隔離、來源／租約／chunk 綁定，不攜帶 Worker 執行權限。
- [x] 來源端點控制面查核：比對 Headscale 節點、單次註冊 key ID、namespace、線上狀態及 overlay IPv4。
- [ ] hcode 檔案來源登錄與 Nodepool 授權；來源位置可變，但檔案與 chunk 身分不可變。
- [ ] Master／CLI 受控提供來源，Worker 依任務租約直接下載與驗證，不經 Nodepool 搬運大檔。
- [ ] Worker 內容快取、容量限制、淘汰、到期與損壞隔離；快取命中仍檢查當次授權。
- [ ] 連線重用、部分下載恢復與 cancellation；有限重試，不得退回未授權來源。
- [ ] 根據已驗證的可用性和快取位置作排程最佳化，不把 Worker 宣告當作永久可用的事實。
- [ ] 上傳／來源續存狀態、CLI 診斷與跨主機故障驗證。

### B. SDK 與可執行工作

- [ ] Python 提交 SDK：登入、artifacts、提交、狀態、等待、取消、預算與錯誤型別。
- [ ] 參數搜尋、固定種子的蒙地卡羅、小型模型批次評分範本及參考答案。
- [ ] 使用現有 lazy dispatch 與 quorum 政策做效能／成本基準；不將低驗證結果標示為已驗證。

### C. 輸出 artifacts 與分片 Job

- [ ] 有界、immutable 的輸出 artifacts，以及與當前 attempt／結果證據的綁定。
- [ ] Nodepool 擁有父 Job、子任務與依賴圖；限制節點數、深度、資料量並拒絕循環。
- [ ] map／reduce、固定分片與合併順序，以及計畫身分綁定。
- [ ] 父 Job 總預算、部分重試、取消傳播、進度與恢復。
- [ ] 小任務合批與資料位置感知，量測真正獨立主機上的加速與網路開銷。

### D. 效能與模型匯入

- [ ] 根據 benchmark 最佳化固定 Rust 算子、分塊與中間 buffer；維持計量與記憶體上限。
- [ ] 受限 ONNX 匯入：白名單算子、shape／dtype／資源驗證、明確拒絕不支援的模型和 custom ops。
- [ ] GPU 模型執行路徑：定義數值及驗證契約後接入，不直接套用 CPU 逐位一致的假設。
- [ ] 若採用專用數學驗證，先建立 Nodepool 權威驗證、輸出承諾和挑戰契約。既有 deterministic Freivalds 診斷不得直接升格為安全證明。

### E. 營運、提供者與恢復

- [ ] 檔案 metadata 與 P2P 內容生命週期分離，清理不破壞活躍租約與正在執行的工作。
- [ ] Nodepool 多實例的派工租約、fencing、重入與結算冪等性；保持單一邏輯權威。
- [ ] 提供者池、抽查、故障域訊號與異常隔離；聲譽僅影響排程，不取代答案驗證。
- [ ] 公開／核准提供者工作範圍，避免把傳輸或磁碟加密宣稱為對提供者保密。
- [ ] TEE 路徑僅在具備實際硬體、遠端證明與金鑰交付驗證時列為完成，不以 stub 或自報能力替代。

## P2P 接線與遷移契約

### 授權和內容分離

現有 `DownloadHcodeChunk` 同時檢查當前副本並讀取 PostgreSQL 中的內容。先增加不讀 bytes 的 `AuthorizeHcodeChunk`，回覆任務已釘選的檔案和 chunk 大小及 SHA-256。授權與舊下載共用同一組資料庫條件，包含 owner、task、worker、execution、attempt、request digest、idempotency、round、replica、protocol、generation、狀態和期限；下載仍在單一 SQL statement 內套用授權，不改成先授權再無條件讀內容。

此回覆只表示當次查詢通過，不是可離線反覆使用的存取權。快取命中也要重新查詢；無法聯絡 Nodepool 時拒絕存取。現有 consensus assignment 的 generation 固定為 `1`，缺失或不同值不被接受。後續代際輪替必須同步修改持久化租約、簽發和驗證，不只放寬驗證器。

### 同儕不可取得完整執行權杖

Worker 的 execution token 只傳給 Nodepool，不轉交來源 peer。P2P 使用另外簽發、只准讀取資料的短效 capability，綁定來源、任務、當前副本和 immutable chunk 坐標；它不得用於回報執行結果、註冊節點或變更帳戶。來源每次讀取前回 Nodepool 驗當前狀態，不能以本機 CAS 或簽章尚未過期取代撤銷檢查。

`auth::artifact_read` 的權杖格式使用 Ed25519、固定 `typ`／`purpose`、來源 UUID audience、租約 UUID、task／artifact ID、檔案與 chunk 雜湊、大小及 offset。效期不超過 60 秒且不設寬限，權杖最多 4 KiB，每次只允許一個最多 1 MiB 的 chunk。簽發上限取 live lease、assignment、artifact 期限的最小值；缺少欄位、額外權限欄位、錯誤來源、非法路徑、非標準雜湊和越界範圍都必須拒絕。

這個 codec 不建立或撤銷租約，也不啟用 peer 傳輸。後續來源登錄必須把 `lease_id` 持久化綁定至當前 Worker、attempt、round、generation、來源與 manifest；Nodepool 每次重新檢查完整綁定，不能把合法簽章當作 live lease。分離 payload 而不是轉交 execution token，是為了讓來源即使取得讀取權杖，也無法拿它回報結果或操作帳戶。

### 可用性不是內容授權

初始拆分保留已存在的 `complete`、`available` 和期限條件，確保舊任務和進行中副本不因遷移中斷。來源登錄接上後，改以有期限的來源狀態決定派工候選，內容完整性仍以 immutable manifest 為錨；不得只因 peer 自稱有檔案就將任務永久標記為已備妥。排程不得在已知無來源時建立執行 hold，來源消失時也不得直接清除進行中的 hold 或跳過正常停止與結算。

來源端點必須經受控的位址和身分驗證；不接受任意 URL、重導向或未驗證 DNS 作為 Worker 下載目標。優先走既有 overlay，獨立本機測試使用顯式隔離配置，不放寬正式路徑。舊 Nodepool 內容僅作既有任務相容讀取，不作新 P2P 任務失敗時的隱藏中央 fallback。

現有 `vpn_peers.virtual_ip` 可由客戶端狀態回報更新，Master 的 VPN client label 也未綁定 `client_identities.identity_id`；兩者都不能直接作為來源端點的可信證據。來源登錄須由 Nodepool 核對已簽發的 enrollment／VPN 身分與 Headscale 控制面中的實際節點及位址，配合受控資料服務埠。未完成綁定的來源維持 pending，不向 Worker 發布；`online=true`、自報 IP 或知道 identity UUID 都不能取代此檢查。

`hivemind_vpn_service::peer_verifier::HeadscalePeerVerifier` 提供有界的控制面查核。只向 operator 設定的 Headscale URL 發出查詢，使用獨立的可重用連線池，不採用環境代理、不跟隨重導向；連線最長 3 秒、整次請求最長 8 秒，回覆最多 64 KiB。節點 ID 只是查詢提示，必須同時符合 Nodepool 已保存的單次 preauth key ID、namespace、線上狀態、節點期限與唯一的 `100.64.0.0/10` IPv4。

新版 Headscale 會遮罩回覆中的 key 文字，因此不能拿它比對原始 secret，也不能接受 peer 自報的 key ID。來源登錄必須先補上簽發時的 key ID 持久化；沒有這個綁定的舊 VPN 紀錄不能直接升格為已驗來源。已消耗的 enrollment key 到期不等於已加入節點到期，節點期限以控制面 `expiry` 為準。這個查核元件尚未接到登錄 RPC；回覆只是一個時間點的身分觀察，不保證資料服務可達或持有任何 chunk。

### CAS 與恢復

既有 `CasChunkStore` 可驗證內容與持久化分塊，但不等於完整快取生命週期管理。加入 hcode 快取時，同時定義容量和清理邊界；損壞內容必須安全隔離或移除後才能重下載，不能假設 `put_chunk` 會覆寫它。磁碟及雜湊工作不得長時間阻塞 async executor；取消後保留已驗證的 chunk，下一次經重新授權後只補缺失內容。

## 依賴與驗收順序

P2P 授權與來源 → 快取與恢復 → SDK／範本 → 輸出 artifacts → 分片 Job 與總預算 → 真實效能驗證 → 算子與受限模型匯入 → 多實例與提供者治理。

GPU、TEE 與跨故障域效能需要相應硬體及環境；未完成實際驗收時必須保留未完成標記。公開網路上的密集跨節點模型平行不是前置功能，先以可獨立切分的工作證明效益。

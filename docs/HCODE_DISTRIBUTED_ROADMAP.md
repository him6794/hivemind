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
- [x] 帳號綁定的 VPN 簽發紀錄：持久化控制面 key ID，重新簽發與撤銷使舊查核失效。
- [x] hcode 來源 metadata 登錄、查詢與撤銷：綁定帳號、VPN enrollment、immutable artifact 及短效期限。
- [x] 逐次 live read lease 與宿主共用來源讀取服務；租約綁定當前副本、來源代際與 immutable chunk。
- [ ] 派工前來源可用性驗證與 Worker 來源查詢；來源位置可變，但檔案與 chunk 身分不可變。
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

這個 codec 本身不建立或撤銷租約；持久化與逐次驗證由下述讀取租約服務處理。`lease_id` 綁定當前 Worker、attempt、round、generation、來源與 manifest；Nodepool 每次重新檢查完整綁定，不能把合法簽章當作 live lease。分離 payload 而不是轉交 execution token，是為了讓來源即使取得讀取權杖，也無法拿它回報結果或操作帳戶。

### 可用性不是內容授權

初始拆分保留已存在的 `complete`、`available` 和期限條件，確保舊任務和進行中副本不因遷移中斷。來源登錄接上後，改以有期限的來源狀態決定派工候選，內容完整性仍以 immutable manifest 為錨；不得只因 peer 自稱有檔案就將任務永久標記為已備妥。排程不得在已知無來源時建立執行 hold，來源消失時也不得直接清除進行中的 hold 或跳過正常停止與結算。

來源端點必須經受控的位址和身分驗證；不接受任意 URL、重導向或未驗證 DNS 作為 Worker 下載目標。優先走既有 overlay，獨立本機測試使用顯式隔離配置，不放寬正式路徑。舊 Nodepool 內容僅作既有任務相容讀取，不作新 P2P 任務失敗時的隱藏中央 fallback。

現有 `vpn_peers.virtual_ip` 可由客戶端狀態回報更新，Master 的 VPN client label 也未綁定 `client_identities.identity_id`；兩者都不能直接作為來源端點的可信證據。來源登錄須由 Nodepool 核對已簽發的 enrollment／VPN 身分與 Headscale 控制面中的實際節點及位址，配合受控資料服務埠。未完成綁定的來源維持 pending，不向 Worker 發布；`online=true`、自報 IP 或知道 identity UUID 都不能取代此檢查。

`hivemind_vpn_service::peer_verifier::HeadscalePeerVerifier` 提供有界的控制面查核。只向 operator 設定的 Headscale URL 發出查詢，使用獨立的可重用連線池，不採用環境代理、不跟隨重導向；連線最長 3 秒、整次請求最長 8 秒，回覆最多 64 KiB。節點 ID 只是查詢提示，必須同時符合 Nodepool 已保存的單次 preauth key ID、namespace、線上狀態、節點期限與唯一的 `100.64.0.0/10` IPv4。

新版 Headscale 會遮罩節點回覆中的 key 文字，因此不能拿它比對原始 secret，也不能接受 peer 自報的 key ID。使用者 VPN 簽發流程從同一份控制面回覆取得單次 key 與正整數 ID，將 `vpn_peers` 和 `vpn_peer_enrollments` 在同一筆資料庫交易保存。綁定包含帳號 UUID、VPN row UUID、控制面 URL、namespace、key ID 和每次重新簽發都會更換的 enrollment UUID；不把 client label 或 `client_identities.identity_id` 當成認證憑證。

Headscale 0.26.1 的 `POST /api/v1/preauthkey` 要求數字 user ID，不接受 namespace 名稱。簽發前由控制面的 user list 查出名稱完全相符的唯一正整數 ID；缺失、重名或非法 ID 都拒絕，不採用客戶端自報 ID 或退回傳送名稱。一般帳號與舊 Worker 簽發路徑共用這個解析。`enrollment_live_headscale_issues_and_persists_numeric_id_keys` 以本機真 Headscale 與隔離 PostgreSQL 驗證簽發、重新簽發和撤銷；此測試預設忽略，須提供測試控制面的 URL／API key 並以 `--ignored` 明確執行，不代表已驗證真節點加入或 P2P bytes 傳輸。

`VpnService::verify_user_vpn_peer` 驗證帳號 JWT 的用途、`sub`／`user_id`、有效帳號及期限，從持久化綁定取得控制面 key ID，再查核實際 overlay 節點。HTTP 查核後重新確認綁定及帳號狀態；中途重新簽發、撤銷、停用或刪除帳號／VPN row 都會使觀察失效。重新建立相同使用者名稱不繼承舊綁定，舊 Worker join 路徑若替換 VPN row 也會原子撤銷原綁定。重新簽發只讓最新提交的綁定有效，不保證並發呼叫的回覆到達順序。

舊 VPN 紀錄不回填可信 key ID，須重新完成已認證的簽發才能走此查核。控制面必須回傳 key ID 並確認 `reusable=false`；缺失時不發布新的 VPN 設定。簽發 JSON 請求不跟隨重導向、回覆限制 64 KiB，錯誤不回傳控制面 body。已消耗的 enrollment key 到期不等於已加入節點到期，節點期限以控制面 `expiry` 為準。

VPN 查核回覆只是一個時間點的身分觀察，不保證資料服務可達或持有任何 chunk，也不建立 artifact read lease。

### 來源登錄與撤銷

`HcodeArtifactSourceService` 的 `RegisterSource`、`GetSource`、`RevokeSource` 只提供任務擁有者的 metadata 管理。登錄請求包含帳號 token、task／artifact ID、VPN client label 和 Headscale node ID；不接受 IP、URL、port、TTL、key ID 或新的檔案雜湊。Nodepool 先檢查擁有者，再經 VPN enrollment 查核 overlay 節點，最後在持久化時重驗帳號、任務、artifact、enrollment 及期限。回覆的 IP 來自控制面，資料服務埠固定為 overlay-only `50054`。

`hcode_artifact_sources` 綁定 artifact row、帳號 UUID、VPN peer row 與 enrollment UUID。每次登錄／續租都更換 `registration_id`，供後續 read lease 拒絕舊代際；單次期限最多 60 秒，並取原帳號 token、節點、artifact 和任務期限的最小值。保存時要求節點觀察不超過 15 秒。每個 artifact 最多 8 個來源，使用 artifact row lock 保證並發配額；到期和已撤銷紀錄可在續登錄時清理。重新簽發 VPN 不會被來源表的 FK 阻擋，也不會透過 cascade 把來源改綁到新的 enrollment。

新 hcode artifact 在任務 admission 時保存擁有者 UUID；來源管理要求此 UUID 與有效帳號一致。舊 metadata 不從 username 自動回填，刪除帳號會清空原 UUID 綁定，重新建立同名帳號不繼承舊 artifact 的來源權限。此限制不改寫舊中央下載契約；需要登錄來源的舊任務須重新提交。

目前已提交的帳號 JWT 沒有可撤銷 session，因此這裡採用明確的 **per-source 撤銷與短效期限**，而不是依賴尚未提交的帳號改版。來源查詢每次重驗帳號 active、原登錄期限、artifact／task 狀態及 enrollment UUID；帳號停用、VPN 撤銷／重簽、來源撤銷與到期均會拒絕後續查詢。這不代表帳號登出能撤銷既有 JWT；之後接入正式帳號 session 時，read lease 必須加上該 session 的 live gate。

登錄狀態固定為 `registered_pending`。它不證明資料服務可達或檔案存在，不改變 artifact `complete`／`availability_status`，不建立 execution hold，不向 Worker 主動發布來源，也不搬運 bytes；`GetSource` 不是下載授權。自動 P2P 派工仍須獨立完成可用性驗證與 Worker discovery。

### 逐次讀取租約與來源服務

`HcodeArtifactReadLeaseService` 由 Nodepool 提供。`IssueReadLease` 接收已知來源 UUID，以及只送給 Nodepool 的 Worker execution token 和 chunk 坐標；它與既有下載共用 current-assignment SQL，另檢查有效帳號 UUID、來源登錄代際、VPN enrollment、task deadline 和持久化的 transfer generation。效期取 60 秒、來源、artifact、task、attempt 及 execution token 期限的最小值，不繼承 execution decoder 的過期寬限。

`hcode_artifact_read_leases` 保存已驗證的 execution claims 而非原 JWT，並綁定來源／登錄／enrollment、artifact row、帳號 UUID、replica row 及完整 manifest scope。每個來源／副本／manifest offset 只保留一筆；重簽更換 lease UUID，舊 capability 隨即失效，重複請求不會無限增加紀錄。`ValidateReadLease` 不只驗簽章；每次以同一 SQL snapshot 重驗 current assignment、持久化租約與來源綁定。先讀取保存的 claims 不構成授權，最後查詢仍要求原 lease UUID 存在。

此階段只允許原本已 `complete`／`available` 的 artifact 和 `ASSIGNED`／`RUNNING` 任務試讀已知來源。授權試讀不代表來源已通過 availability probe，也不將 pending-only artifact 變成可派工。原 CLI 上傳和 Worker 自動下載路徑尚未切換；新 P2P-only admission、探測、來源選取與 overlay 接線仍是後續獨立工作。

`hivemind-artifact-peer` 提供 CLI／Master／Worker 可共用的宿主服務，不依賴 Worker executor。來源啟用前持有與已驗證整檔及 chunk 雜湊相同的 immutable bytes，不依遠端路徑重新開檔。每個宿主最多 16 個來源、單檔 64 MiB、合計 256 MiB；同時最多 8 次讀取，單次最多 1 MiB。宿主可更新同一來源的登錄代際，不重新載入內容、不重啟 listener；遠端 RPC 沒有此操作。

來源只接受用途隔離的 read token，每次回 operator 設定的 Nodepool channel 驗證，最多等待 5 秒，不快取成功授權。Nodepool 不可達、租約失效、登錄代際不一致或範圍不符均不回傳 bytes；網路 await 後再驗期限。金鑰沿用平台 Ed25519 key pair，以固定 typ、purpose 及不含 account／execution 欄位的 schema 隔離用途；Nodepool 啟動時檢查私鑰、公鑰是否匹配。來源只持有公鑰。

撤銷以 Nodepool 的逐次檢查為界：撤銷提交後開始驗證的讀取會拒絕；已通過檢查、正在傳送的單個 chunk 可能完成，不能追回已交付資料。租約與來源失效不刪 execution hold，也不變更停止、結果驗證或結算流程。真 TCP gRPC 與隔離 PostgreSQL 測試涵蓋直接讀取和失效條件；這不等於跨主機 overlay 或效能驗證。

### 首次端到端 P2P 接線

- 節點識別在完成 VPN 加入後解析。簽發 preauth key 時尚無新節點 ID，不能把 key ID 當成 node ID；Nodepool 須依已保存的 enrollment 向控制面確認實際節點。
- 來源服務放在 CLI、Master 與 Worker 可共用的宿主層，不讓 CLI-only build 依賴 Worker 執行器。Windows userspace overlay 須另接 `50054` inbound forwarding，不能假設系統介面能直接 bind overlay IP。
- Worker 的執行讀取租約綁定當前 assignment、來源 `registration_id`、enrollment 與 immutable chunk；來源每次服務前回 Nodepool 重驗。原 Worker execution token 不出現在 peer 請求中。
- 派工前的可用性探測與執行讀取使用不同用途及授權條件，不能偽造 Worker assignment 來解開依賴循環。若以 chunk SHA-256 驗證探測，必須驗完整的 manifest chunk（最多 1 MiB），不能拿部分 bytes 的雜湊與整個 chunk 比對。一次成功探測只形成有期限的可用性觀察，不代表檔案永久完整或所有 chunks 均可取得。
- 新 P2P 任務明確記錄傳輸模式；來源不可用時等待、重試或失敗，不自動改讀 Nodepool bytes。舊中央儲存路徑仍維持既有任務相容性。

### CAS 與恢復

既有 `CasChunkStore` 可驗證內容與持久化分塊，但不等於完整快取生命週期管理。加入 hcode 快取時，同時定義容量和清理邊界；損壞內容必須安全隔離或移除後才能重下載，不能假設 `put_chunk` 會覆寫它。磁碟及雜湊工作不得長時間阻塞 async executor；取消後保留已驗證的 chunk，下一次經重新授權後只補缺失內容。

## 依賴與驗收順序

P2P 授權與來源 → 快取與恢復 → SDK／範本 → 輸出 artifacts → 分片 Job 與總預算 → 真實效能驗證 → 算子與受限模型匯入 → 多實例與提供者治理。

GPU、TEE 與跨故障域效能需要相應硬體及環境；未完成實際驗收時必須保留未完成標記。公開網路上的密集跨節點模型平行不是前置功能，先以可獨立切分的工作證明效益。

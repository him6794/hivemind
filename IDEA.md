# HiveMind — 分散式運算平台 · 構想整理

## 一、系統定位

**HiveMind** 是一個 P2P 式分散式運算平台，讓使用者（Master）將 Python 運算任務外包給網路上提供算力的節點（Worker），由 NodePool 作為調度中樞，並以 CPT 代幣作為算力交換的經濟激勵。

---

## 二、架構總覽

```
[Master] ──上傳任務(.torrent)──▶ [NodePool] ──分配任務──▶ [Worker]
[Master] ◀──查詢結果──────────── [NodePool] ◀──回報輸出── [Worker]
```

- **通訊協定**：gRPC（使用 `hivemind.proto` 定義）
- **網路層**：Tailscale 虛擬內網（簡化連線、提升安全性）

---

## 三、節點職責

### Master
- 提供使用者上傳任務的介面
- 上傳任務的 `.torrent` metadata（非任務本體）至 NodePool
- 查詢任務狀態與執行結果
- 持有 CPT 帳戶，依資源使用量支付給 Worker

### NodePool（調度核心）
- 接收 Master 的任務請求
- 管理 Worker 的註冊、心跳與資源狀態（超時判定：**30 秒**無回應視為失聯）
- 根據資源分數（CPU/GPU/RAM）挑選最適 Worker
- 將 `.torrent` metadata 下發給 Worker 及暫存節點
- 執行 CPT 轉帳計算與發起（每分鐘一次）
- 處理任務失敗重派邏輯

### Worker
- 向 NodePool 登入註冊
- 定期回報資源計分（CPU 分數、GPU TOPS、可用 RAM），每 **30 秒**一次
- 接受任務後透過 BitTorrent 協定下載任務包
- `.torrent` metadata 由 **Master 端產生**後上傳至 NodePool
- 呼叫 Rust Executor 執行任務，主動限制資源
- 執行期間即時回報輸出與資源使用情況
- 持有 CPT 帳戶，接收來自 Master 的報酬

---

## 四、任務包設計

| 項目 | 說明 |
|------|------|
| **格式** | Python 程式 zip 包，含 `main.py` 作為入口 |
| **依賴** | zip 包內含必要資源與相依套件 |
| **分發方式** | 僅上傳 `.torrent` metadata；實際檔案由 Worker 透過 BitTorrent 下載 |
| **暫存** | NodePool 將 `.torrent` 下發給多個節點做暫存或轉發 |
| **執行器** | 目前以 [pydantic/monty](https://github.com/pydantic/monty) 為基礎，由 Rust 實作沙箱 |

---

## 五、資源管理

### 資源計分
- **CPU 分數**：浮點數，由 Worker 計算後回報
- **GPU 分數**：浮點數，由 Worker 計算後回報
- **可用 RAM**：實際可用清單，供 NodePool 精準分配

### 資源限制
- 由 **Rust Executor** 主動限制任務的資源使用上限
- 確保隔離性與系統穩定

---

## 六、GPU 支援策略

- **短期**：優先支援 NVIDIA GPU
- **長期**：以 C++ 擴充層將不同廠商 GPU 統一為一致介面
- Rust Executor 透過統一介面呼叫 GPU，達到跨廠商支援
- Python 任務可透過 Rust Executor 調用 GPU 計算能力

---

## 七、CPT 代幣經濟

### 用途
- 系統內部虛擬代幣，作為算力交換的媒介
- Master 使用 CPT 支付任務費用
- Worker 完成任務後獲得 CPT 報酬

### 取得方式
- 提供算力執行任務（賺取）
- 開源社區貢獻（賺取）
  - 需定義貢獻認定標準與審核流程（TBD）

### 轉帳機制

```
每 1 分鐘 NodePool 檢查一次 → 確認任務正常運行 → 從 Master 帳戶轉帳至 Worker 帳戶
```

| 情境 | 行為 |
|------|------|
| 任務正常運行滿 1 分鐘 | NodePool 從 Master 轉帳給 Worker |
| 任務在 1 分鐘內完成（正常） | 立即轉帳 |
| Master 帳戶餘額不足 | 通知 Worker 停止該任務 |
| Worker 超時導致失敗 | 不轉帳；任務重新派發給其他 Worker |
| 任務本身程式錯誤導致失敗 | 不轉帳；**不重新派發** |
| 任務正在執行但未滿 1 分鐘 | 不轉帳，等下一個周期 |

### 轉帳金額計算
- 由 Master 申請的資源量（CPU/GPU/RAM）決定計費標準
- 計算與執行皆在 NodePool 進行

### 費率規格（每分鐘計費）
費率是固定的，申請多少就支付多少，沒有動態調整
| 資源 | 計費基準 | 單價 | 說明 |
|------|---------|------|------|
| **CPU** | 每分鐘從 10¹³ 往後能找到的質數數量 | +0.01 CPT / 質數 | 以質數計算作為 CPU benchmark；數值越高代表 CPU 越強 |
| **GPU** | TOPS（Tera Operations Per Second） | 1 CPT / TOPS | 申請時填報|
| **RAM** | 從 128 MB 起，每 128 MB 為一單位 | +0.01 CPT / 單位 | 例：1 GB ≈ 0.08 CPT/min |


---

## 八、技術棧

| 技術 | 用途 |
|------|------|
| **Go** | Master、NodePool、Worker 核心服務邏輯 |
| **Rust** | 任務執行器（沙箱、資源限制、效能優化）；所有計算密集邏輯 |
| **C++** | GPU 驅動/介面擴充層（跨廠商統一介面） |
| **React** | 前端介面（Web UI / Electron WebView 桌面應用） |
| **Redis** | 任務狀態與節點狀態的快速儲存與查詢 |
| **Kafka** | 任務分發與工作流的消息隊列 |
| **PostgreSQL** | 帳號資料、CPT 餘額、任務日誌的長期儲存 |
| **BitTorrent** | 任務包的 P2P 分發機制 |
| **Tailscale** | 節點間虛擬內網，簡化 NAT 穿透與安全性 |

---

## 九、實作路線圖

### Phase 1 — 核心基礎功能
- [ ] 登入 / 註冊系統（帳號 + CPT 帳戶）
- [ ] Master：上傳任務（.torrent）、查詢任務狀態與結果
- [ ] NodePool：節點註冊、心跳管理、任務排程與分配
- [ ] Worker：註冊、資源回報、下載任務包、回報輸出
- [ ] Rust Executor：沙箱執行 Python 任務、資源限制
- [ ] CPT 轉帳流程（NodePool 每分鐘計算與執行）

### Phase 2 — 前端介面
- [ ] Worker 管理介面（狀態監控、任務紀錄、CPT 餘額）
- [ ] Master 任務提交介面（上傳、追蹤、結果下載）
- [ ] 使用 Electron WebView 包殼為桌面應用程式
- [ ] 官方網站（Landing Page）

### Phase 3 — GPU 支援
- [ ] NVIDIA GPU 支援（優先）
- [ ] C++ 統一 GPU 介面層
- [ ] Rust Executor 調用 GPU 介面
- [ ] Rust Executor 支援更多 Python 模組

### Phase 4 — 網路強化
- [ ] Tailscale 連線整合（節點自動組網）

### Phase 5 — 擴充與優化
- [ ] 其他廠商 GPU 支援（AMD、Intel Arc 等）
- [ ] 更多 Executor 類型（非 Python 任務）
- [ ] Object Storage + Signed URL 作為任務分發備案
- [ ] 監控與告警系統

---

## 十、帳號系統

- 自建帳號系統（不依賴第三方 OAuth）
- 需實作：註冊 / 登入、密碼雜湊（bcrypt/argon2）、JWT Token 驗證
- PostgreSQL 儲存帳號資料與 CPT 餘額
---
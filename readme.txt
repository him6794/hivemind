這是一個分散式運算平台（主從式架構），主要由三種節點組成：master、node_pool 與 worker。

**系統概述**
- **架構說明**: master 與 worker 都與 node_pool 通訊；node_pool 為系統的調度核心，負責節點管理與任務分配。
- **通訊方式**: 節點間以 gRPC 通訊，並建議使用 Tailscale 建立虛擬內網以簡化網路連線與安全性。

**節點職責**
- **Master**: 提供使用者上傳任務與取得結果的介面；僅負責任務上傳與結果查詢。
- **NodePool**: 接收 Master 的任務，根據資源與需求選擇合適的 Worker，並負責節點註冊、狀態追蹤與資源分配。
- **Worker**: 登入註冊至 NodePool，定期回報資源與狀態，下載並執行分配到的任務，執行期間回報任務輸出與資源使用情況。

**任務包與分發**
- **任務格式**: 任務以 Python 程式包（zip）打包，須包含一個 `main.py` 作為入口，以及其他必要資源與相依套件。
- **執行環境**: 目前採用 monty（https://github.com/pydantic/monty）執行任務，未來會擴充以支援其他執行器。
- **分發機制**: 任務檔不直接上傳至 NodePool；僅上傳 .torrent metadata，NodePool 會將 .torrent 下發給多個節點做暫存或轉發，最終由執行該任務的 Worker 下載實際檔案。

**資源回報與限制**
- **資源計分**: Worker 會計算 CPU/GPU 分數（浮點數）並提供實際可用 RAM 清單，供 NodePool 做精準分配。
- **資源限制**: 任務的資源使用上限由 Rust 實作的 Python 執行器主動限制（以確保隔離與穩定）。

**GPU 支援策略**
- **統一介面**: GPU 呼叫以 C++ 擴充方式將不同廠商的 GPU 統一為一致介面，供 Rust 中的 Python 執行器呼用，以達到跨廠商支援。

**技術棧**
- **Go**: NodePool 與 Worker 與master的核心服務邏輯。
- **Rust**: 任務執行器（含資源限制、隔離與效能優化）基於pydantic-monty。及所有需要計算的邏輯
- **C++**: GPU 驅動/介面擴充層。
- **React**: 前端介面（提供 Web UI / WebView）。
- **Redis**: 任務狀態與節點狀態的快速儲存與查詢。
- **Kafka**: 任務分發與工作流程的消息隊列。
- **PostgreSQL**: 帳號資料與任務日誌的長期儲存。

實作順序:
基礎功能實作
確保其能夠登入註冊、上傳任務、分發任務、執行任務並回報結果、cpt轉帳等。

前端實作
提供worker與master的web ui,並使用webview包殼成桌面應用程式。
實作官網

gpu先支援nvidia的gpu,並提供統一的呼叫介面。
實作rust的python執行器調用gpu介面。
實作rust的python執行氣的更多模組支援。

實作tailscale的連線功能。

cpt代幣是一種系統內部的虛擬代幣，用於支付任務的費用。用戶可以通過完成任務獲得cpt代幣，或者購買cpt代幣來支付任務費用。
cpt代幣將由每個worker在執行任務中由master帳戶轉帳給worker帳戶，轉帳金額由master申請的資源來計算
轉帳流程將在nodepool中運行，金額及轉帳都在nodepool中計算與執行，每次轉帳前都要確保task正常運作才轉帳，若master帳戶餘額不足將會通知worker停止該任務，若worker執行的任務錯誤將會停止並更新任務狀態。
任務若因為worker超時而導致任務失敗，將會將該任務重新派發給其他worker執行，並且不會從master帳戶轉出cpt代幣。
若為程式本身錯誤導致任務失敗，將不會重新分發任務

轉帳為每1分鐘一次，若未達到1分鐘則不會轉帳，若任務在1分鐘內完成則會立即轉帳(非錯誤執行)。

---

Worker 與 Rust executor-cli 串接（最新）

- Worker 會優先使用 `WORKER_EXECUTOR_CMD`。
- 若未設定 `WORKER_EXECUTOR_CMD`，且 `WORKER_EXECUTOR_AUTO_RUST=true`（預設），則會自動嘗試執行 `executor-cli`。
- 可用 `WORKER_EXECUTOR_RS_BIN` 覆寫預設二進位名稱。

常用環境變數：

- `WORKER_EXECUTOR_CMD`：完整命令列（例如：`executor-cli` 或 `C:\\path\\executor-cli.exe`）
- `WORKER_EXECUTOR_AUTO_RUST`：是否啟用自動 Rust CLI（預設 `true`）
- `WORKER_EXECUTOR_RS_BIN`：自動模式時的 CLI 名稱/路徑（預設 `executor-cli`）
- `WORKER_EXECUTOR_TIMEOUT_SEC`：worker 執行外部 executor 的 timeout（秒）
- `WORKER_USAGE_REPORT_INTERVAL_SEC`：worker 上報 task usage 週期（秒）

executor-cli 環境變數：

- `EXECUTOR_STEPS`
- `EXECUTOR_STEP_LOG_INTERVAL`
- `EXECUTOR_STRICT_SOURCE`
- `EXECUTOR_OUTPUT_FORMAT`（`result-url` 或 `magnet`）
- `EXECUTOR_FORCE_FAIL`

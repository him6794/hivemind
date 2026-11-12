# 第三階段同機端到端驗證

`scripts/phase3_e2e.py` 使用一個本機 Nodepool、一個本機 Master，以及三個本機原生 Worker 驗證管線和模型切分。三個 Worker 使用三個獨立測試帳號，任務要求三副本、二票共識。這是同一台機器上的整合測試，不是跨主機或公開網路效能證據。

需要 Python 3.10 以上、OpenSSL、Docker，以及已建置的完整 `hivemind-bin`。先在儲存庫根目錄建置並檢查依賴：

```powershell
cargo build --manifest-path hivemind-rs/Cargo.toml -p hivemind-bin --bin hivemind-bin
docker pull postgres:16-alpine
docker pull redis:7-alpine
python scripts/phase3_e2e.py --check-only
```

`--check-only` 不建立檔案或啟動服務。執行驗證時，結果目錄必須不存在或為空：

```powershell
python scripts/phase3_e2e.py --out-dir test_logs/phase3-local
```

非預設執行檔位置可用 `--binary` 指定。執行前以 `docker context inspect` 確認目前使用本機 Docker engine，不要使用遠端 Docker context。Worker 預設採用本機路由的 IPv4 位址；如有多張網卡，可以用 `--worker-host` 指定本機位址。Nodepool 與 Master 的連線、資料庫與 Redis 對外連接埠都使用 loopback；Worker 的 gRPC 需要本機非 loopback 位址才能符合現有的 Worker 位址驗證。

腳本會依序驗證：

1. 一條有兩個平行項目及後續合併階段的管線，在本機與經過 Master 的實際提交都得到 `13`。
2. 由腳本產生的 F32／F64 safetensors 權重可經 `model init`、`model split` 切成多階段及多個列分片。管線在本機的結果與獨立純量參考計算在 `1e-12` 內一致。
3. 切出的模型透過 `pipeline run` 提交，上傳權重分片，遠端任務結果與本機管線結果完全一致。
4. 每個任務都完成及結算，具有三副本共識資料；三個 Worker 各至少有一筆成功、正用量且帶回報時間的副本紀錄，不能只靠指派紀錄通過。權重 artifacts 必須完整上傳。

`summary.json` 記錄執行檔 SHA-256、主機、程序 PID、同機註記、每階段與每任務的用量／計費／時間，以及最後結果。時間包含 Nodepool 的任務建立至完成延遲、Worker 回報的執行時間，以及 CLI 完整執行耗時；這三種時間分開保存。`replicas.json` 保存各 Worker 的實際指派與回報，`artifacts.json` 保存權重分片的摘要與大小。各服務與 CLI 的 stdout／stderr 另存為紀錄檔。

腳本每次建立專用 PostgreSQL 與 Redis 容器，使用隨機密碼及新產生的簽章金鑰，測試帳號只存在該次資料庫。它會排除繼承的服務設定、帳號憑證與 HTTP 代理設定，在儲存庫之外建立工作目錄，並用空白 `.env` 阻止完整執行檔載入上層目錄設定。服務不會自動開啟 UI 視窗。結束時只停止本次建立的程序，按本次記錄的確切容器 ID 刪除容器與其測試資料，保留結果目錄。若中途被外部中斷，`resources.json` 記錄本次容器 ID、程序 PID 與暫存目錄，可據此核對存活狀態後清理。

驗證腳本自身的報表、隔離設定與清理邏輯（不啟動服務）：

```powershell
python -m unittest discover -s scripts -p test_phase3_e2e.py -v
```

實際執行結果應另記入第三階段計畫，附上結果路徑、執行檔摘要與確切日期。腳本本身或 `--check-only` 成功不算端到端驗收通過。

## 2026-11-12 最終版本實跑結果

包含 P3／P6 審查修正的本機 `d65b8f38` 完整 debug binary，於 **2026-11-12 00:59:28–00:59:57 UTC（臺北 08:59:28–08:59:57）** 通過。執行檔 SHA-256：

```text
16f80c6475829fc0a41c05fde97665a0f4d904a21e1c2de751da04bbccd4d685
```

完整本機報告：`test_logs/phase3-e2e-20261112-reviewed/summary.json`。移除主機位址與程序識別資訊後的可版本控制證據：[phase3-same-host-2026-11-12.json](evidence/phase3-same-host-2026-11-12.json)。每任務的共識、用量、計費、建立與完成時間保留在該檔案中。

| 管線／階段 | 任務數 | 用量單位合計 | 計費 CPT | 該階段最長單任務延遲（毫秒） |
|---|---:|---:|---:|---:|
| 一般管線／1 | 2 | 162 | 178 | 269.372 |
| 一般管線／2 | 1 | 66 | 72 | 242.992 |
| 切分模型／1 | 3 | 5,766 | 6,341 | 1,284.583 |
| 切分模型／2 | 1 | 1,872 | 2,059 | 2,716.297 |

用量為 Nodepool 任務層級的副本用量合計，計費包含平台費。延遲為 Nodepool 建立任務至完成的時間，不是純計算時間，也不是整個階段耗時。Worker 的毫秒時間欄位在這次全部為 `0`，不據此宣稱運算速度。

- 一般管線最後輸出 `13`；三任務共 228 單位、250 CPT；完整 CLI 耗時 3.500 秒。
- 模型有兩階段、第一階段三個列分片；四任務共 7,638 單位、8,400 CPT；完整 CLI 耗時 6.641 秒。遠端與本機輸出完全一致，並通過獨立純量參考 `1e-12` 比較。
- 七個任務全數完成並結算，皆為三副本、二票 quorum。21 筆副本回報全數成功且有正用量與時間戳；每個 Worker 各 7 筆。四個權重 artifact 全部完成，共 3,248 bytes。
- 五個測試程序、兩個專用容器及暫存工作目錄已清除；`resources.json` 無清理錯誤，另以程序、容器與路徑查詢確認。刪除的是可重建的測試資料，報告及紀錄保留。

先前 00:34 UTC 的初次 E2E 也通過，但使用審查修正前的 binary；最終驗收以此處的摘要與證據為準。本次仍只證明同機 CPU hcode 整合，不證明跨主機、公網、Linux headless 閒置判斷、GPU、一般計算隔離或效能。

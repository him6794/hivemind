# 管線、模型切分與閒置分享

管線由本機 CLI 編排，每個項目仍是獨立的 CPU hcode 任務，由 Nodepool
派發副本、驗證與結算。執行時 CLI 必須保持開啟；中斷後可用狀態檔續跑。
這些命令屬於 Master 的工作流程，官網僅提供說明。

## 管線格式與本機檢查

把以下內容存為 `pipeline.json`，同目錄建立兩個原始碼檔案。

```json
{
  "input": 4,
  "stages": [
    {"items": [
      {"source": "add.hcode", "params": 2, "max_cpt": 100},
      {"source": "add.hcode", "params": 3, "max_cpt": 100}
    ]},
    {"items": [{"source": "sum.hcode", "max_cpt": 100}]}
  ]
}
```

`add.hcode`：

```hcode
return get(input, "previous")[0] + get(input, "params");
```

`sum.hcode`：

```hcode
return get(input, "previous")[0] + get(input, "previous")[1];
```

```powershell
hivemind-bin pipeline check ./pipeline.json
```

結果為 `[13]`。第一階段兩個項目各得到 `previous: [4]`，第二階段得到
`previous: [6, 7]`。同階段的項目可在網路上同時執行，但傳給下一階段的
輸出永遠維持項目順序。函式以 `return` 回傳 JSON 值；`print` 的文字不是
下一階段的有效輸入。
頂層字串在 hcode 中會輸出原始文字；若要傳遞字串，請包在清單或物件內，
讓結果保持有效 JSON。

所有路徑相對於管線檔。固定參數可用 `params` 或 `params_file`，兩者擇一；
權重以 `"artifacts": {"model": "weights.safetensors"}` 指定。
`check` 本機執行整條管線，列出用量、副本所需上限及輸出大小，並檢查
1 MiB 任務輸入及 256 KiB 共識結果限制。`--replicas` 可設定 2 至 7，
預設為 3；`model split`、`pipeline check` 與 `pipeline run` 請使用相同副本數。

## 送出、預算與續跑

先在本機環境設定 `HIVEMIND_PASSWORD`，再執行：

```powershell
hivemind-bin pipeline run ./pipeline.json --api http://127.0.0.1:8082 --username alice --replicas 3 --max-total-cpt 300 --task-prefix demo --out ./results.json
```

`--max-total-cpt` 必填。有 `max_cpt` 的項目使用指定上限；缺少時在送出前
本機試跑，以上限涵蓋選定副本的用量與平台費，且不低於 Master 的資源報價。
`--no-local-run` 要求每個項目都寫好上限。實際費用由 Nodepool 決定。

每一階段送出前檢查所有歷次已結算金額、未結算任務的上限，以及這一階段
待送項目的上限。加大預算不會抹去先前失敗嘗試的費用。觀察到失敗時停止
送出後續項目；已送出的其他任務仍可能完成或計費，CLI 不會自動取消它們。

任務 ID 從 `demo-1-1` 起，失敗後重送用 `demo-1-1-r1`、`-r2`。
未指定 `--task-prefix` 時會產生並印出前綴。每次送出前保存
`results.json.state.json`。續跑必須使用同一前綴、結果路徑、帳號、API、
資源與副本設定：

```powershell
hivemind-bin pipeline run ./pipeline.json --api http://127.0.0.1:8082 --username alice --replicas 3 --max-total-cpt 600 --task-prefix demo --out ./results.json --resume
```

續跑先查詢先前任務；完成項目不重送，回應遺失的項目先確認是否已存在。
輸入、原始碼、參數或權重內容改變會被拒絕；可提高管線的 `max_cpt` 與
總額，已送出任務的上限不變。不要手動修改狀態檔。

只要曾嘗試送出任務，不論成功、拒絕、失敗或等待逾時，都會寫出 JSON
結果檔，包含 `success`、`final_output`、`billed_cpt`、
`settled_or_reserved_cpt`、`error` 與逐項逐次的 `rows`。
在送出前就被預算或檔案檢查擋下的新執行，不會留下狀態檔。
`--timeout-secs` 是每一階段的等待上限，預設 3600 秒；逾時後可續跑。

## 從 safetensors 產生模型與切片

支援依序命名的 `N.weight`（`[out, in]`）、選填 `N.bias`（`[out]`），
以及 F32／F64 權重。模型描述支援 `linear`、`relu`、`gelu`、`softmax`。
權重檔沒有活化函式資訊，請明確選擇隱藏層及輸出層活化函式：

```powershell
hivemind-bin model init ./weights.safetensors --activation relu --final softmax --out ./model.json
hivemind-bin model split ./model.json --weights ./weights.safetensors --input (Get-Content -Raw ./input.json) --out-dir ./split-model
hivemind-bin pipeline check ./split-model/pipeline.json
```

`--input` 接受 JSON 文字，不是檔案路徑；上例由 PowerShell 讀取 `input.json`
的內容。檔案是一批樣本，例如 `[[1.0, 2.0], [3.0, 4.0]]`；最後輸出也以
樣本為列。模型描述中的權重形狀必須前後相容。切分時會量測權重的 JSON
文字大小與實際執行的配置量，把放得下的相鄰層放在同一項目；大層按輸出
列分配到平行項目，再合併。Softmax 會在完整的列合併後執行。

輸出目錄包含每個項目的 hcode、僅供該項目使用的權重分片及 `pipeline.json`。
切分器會以樣本輸入本機跑完整條管線並寫好上限。`--max-tensor-bytes`、
`--max-materialization-bytes` 可設定較緊的切分預算；仍放不下時會指出層與原因。
接著使用 `pipeline run` 送出，帶上足夠的總預算。相同形狀下，數值變化仍
可能增加 JSON 大小，預留餘裕不代表接受任意輸入。

這是批次 CPU 前饋網路，不是逐 token 互動式語言模型或 GPU 切分。
每段都有派工與副本共識開銷；權重會送到被指派的 Worker，不具保密性。
切分功能沒有改變 CPT 計價。

目前的數值契約使用能精確往返的 binary64 JSON 解析，避免中間結果在跨階段
傳遞時發生額外捨入。這項更新及 `concat` 都改變了 active hcode 的語意摘要；
CLI、Nodepool 與 Worker 必須使用相容的同版執行檔，不能混用舊摘要。
歷史 frozen v0 契約保持不變。

## 聯邦平均訓練範例

`examples/hcode/federated-training/` 包含只用 Python 標準函式庫的資料產生器、
softmax 迴歸、本機梯度下降及平均階段。預設四回合，每回合三份資料分片
各自訓練，接著平均權重；下一回合以上一回合的平均值開始。

```powershell
python examples/hcode/federated-training/generate.py --help
hivemind-bin pipeline check examples/hcode/federated-training/pipeline.json
```

執行方式與固定預期值見範例 README。資料為合成資料，不需下載資料集；
測試比較獨立參考計算與逐回合損失。

## Worker 閒置分享

Worker UI 的分享設定可切換 `always`（預設）及 `idle`，並設定閒置秒數
（預設 300 秒）與 Linux CPU 門檻（預設 20%）。Windows 依目前登入工作階段
最後一次鍵盤或滑鼠輸入判定；Linux 使用系統 CPU 使用率，必須持續低於
門檻達到設定時間。缺少有效或足夠新的取樣時不視為閒置。介面會標示目前
判斷來源與是否可接工作。

不符合閒置條件時，心跳回報 `BUSY`；符合時回報 `IDLE`。更新透過一般心跳
週期（約 10 秒）送達。Worker 另外在本機拒絕新任務執行與新的權重準備，
已在執行的任務會完成；執行中的重複請求可加入等待，已快取的結果可重取。

`BUSY` 不是 Nodepool 的排除派工保證：現有共識選樣刻意不依 Worker
自報忙碌狀態排序，仍可能選到這台 Worker。本機拒收不會改變副本數、共識
或結算規則；若剩餘副本不足，該任務可能因無共識失敗，而其他副本已完成的
有效用量仍可能計費。CLI 可依管線續跑規則再試，沒有透明自動補派保證。

UI 儲存的設定會保留至重新啟動後，
優先於啟動環境變數。本機 `/api/sharing` 的 GET／POST 需要登入並由 Nodepool
驗證 Worker 擁有者；無法完成驗證時不提供讀取或修改。

完整驗收及同機端到端紀錄見 `NEXT_VERSION_PHASE3_2026-11-11.md`。

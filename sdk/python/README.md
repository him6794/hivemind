# hivemind-sdk（Python）

呼叫 Master HTTP API 的 Python 客戶端。只用標準函式庫，沒有外部相依套件，需要 Python 3.9 以上。

## 安裝

```bash
pip install ./sdk/python
```

## 認證

用 API token（建議）或登入後的 token。Token 可以傳給 `Client`，也可以設定環境變數 `HIVEMIND_TOKEN`。SDK 只把 token 放在 `Authorization` 標頭，`repr(client)` 不會顯示它。

```python
from hivemind_sdk import Client, write_results_csv

client = Client("http://127.0.0.1:8082", token=None)  # None 時讀取 HIVEMIND_TOKEN
```

建立 API token 需要登入會話；用 API token 不能再建立或撤銷 token：

```python
created = client.create_token("nightly", expires_in_days=30)
print(created["api_token"])  # 只會回傳一次，請妥善保存
client.revoke_token(created["token_id"])
```

## 執行一個函式

```python
source = "let x = get(input, \"n\");\nx * 2;"
report = client.dry_run(source, '{"n": 21}')       # 本機試跑，不扣款
if report["success"]:
    print(report["output"], report["quote"]["required_max_cpt"])
task = client.submit("double-21", source, '{"n": 21}', max_cpt=report["quote"]["required_max_cpt"])
```

## 批次：伺服器端的 Job

Job 由 Nodepool 執行。建立之後，即使腳本結束，Job 仍會繼續跑。

```python
job = client.create_job(
    source,
    [{"n": n} for n in range(1000)],
    per_item_max_cpt=10,      # 每筆的上限，含平台費
    job_max_cpt=5000,         # 整個 Job 的上限；省略時為 per_item_max_cpt × 筆數
    name="sweep",
)
final = client.wait_for_job(job["job_id"], poll_seconds=10)
print(final["state"], final["completed"], final["failed"], final["refused"], final["spent_cpt"])

for item in client.iter_job_items(job["job_id"]):
    print(item["index"], item["state"], item["billed_cpt"], item["output_preview"][:40])

write_results_csv(client, job["job_id"], "results.csv")  # 每筆一列；試算表不會執行公式
```

每筆輸入會變成一個任務，任務 ID 是 `job-<Job 前 12 碼>-<序號>`。Nodepool 只在整個 Job 的花費加上正在執行的上限不超過總上限時才提交下一筆；若餘額不足，Job 會等待並在 `note` 說明原因。`client.cancel_job(job_id)` 會取消尚未開始的輸入，並要求正在執行的任務停止。

`item["output_preview"]` 只含前 4096 個字元；完整輸出用 `client.task_log(item["task_id"])` 取得。

## 每日免費額度

Nodepool 可以開啟每日免費額度（`ACCOUNT_DAILY_ALLOWANCE_CPT`，預設 0，即關閉）。開啟後，每個帳號每 24 小時可領取一次，額度從系統池撥出；系統池餘額低於下限（`ACCOUNT_ALLOWANCE_POOL_FLOOR_CPT`）時暫停發放。API token 也能領取，因為額度記在帳號上。

```python
grant = client.claim_allowance()   # 未開放、已領取過或系統池不足時丟出 HivemindError（409），message 說明原因
print(grant["granted_cpt"], grant["balance"], grant["next_claim_at"])
```

## 錯誤

所有 HTTP 錯誤與網路錯誤都會丟出 `HivemindError`：

- `status`：HTTP 狀態碼；無法連到 Master 時為 `0`。
- `message`：伺服器回傳的訊息。

## 測試

```bash
cd sdk/python && python -m unittest discover -s tests
```

測試用本機的假 Master，不需要真實的 Nodepool。

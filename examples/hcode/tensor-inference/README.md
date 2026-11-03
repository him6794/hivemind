# 分塊載入權重並執行線性推理

這個範例用標準 Python 產生約 1.2 MB 的 safetensors 檔案，再透過 CLI 上傳到 Nodepool。Worker 驗證完整檔案後，hcode 只解碼最後四個元素，以矩陣乘法計算 `[[1,2],[3,4]] × [1,2]`。

這是人工權重的傳輸與運算範例，不是預訓練模型，也不是完整 LLM 引擎。它示範如何讓檔案大於 1 MiB JSON 上限，並保留解碼後的張量大小限制。

## 執行

需要已啟動的 Master、使用相同 hcode 語意版本的 Nodepool 與足夠的 Worker，以及有 CPT 餘額的帳號。從此目錄執行：

```powershell
python generate_weights.py weights.safetensors
hivemind-bin submit inference.hcode --task-id tensor-demo-001 --api http://127.0.0.1:8082 --username alice --password "$env:HIVEMIND_PASSWORD" --input "[1.0,2.0]" --artifact model=weights.safetensors --max-cpt 10000
hivemind-bin status tensor-demo-001 --api http://127.0.0.1:8082 --username alice --password "$env:HIVEMIND_PASSWORD"
```

先把 `HIVEMIND_PASSWORD` 設成測試帳號的密碼。`--password` 的值會出現在程序參數中；請只在信任的本機環境使用。產生器拒絕覆寫既有檔案；每次提交也請使用新的 task ID。

任務完成後的 output 應為：

```json
[5.0,11.0]
```

`model` 是 `--artifact` 指定的邏輯名稱；`weights` 是檔案內的張量名稱。`299996` 是元素偏移量，不是位元組偏移量。這裡的切片連續且採 row-major 排列。

權重會送到被指派的 Worker；請勿以此流程提交不能向運算提供者揭露的私有資料。其他大小、配額與授權限制見 [hcode runtime 契約](../../../docs/HCODE_RUNTIME.md#tensor-artifact-inputs-cpu-hcode)。

# 小型聯邦平均訓練

這個範例用一般 hcode 管線訓練三類 softmax 迴歸。每輪先讓三個平行項目各自從相同的全域權重做三步梯度下降，再由下一階段的一個項目平均權重。預設跑四輪，也可以產生單輪管線。

資料完全是合成的：三個分片各有六筆樣本，每類兩筆。兩個特徵加上固定的 `1.0`，因此權重矩陣的第三列就是偏置。各分片樣本數相同，所以等權平均就是依樣本數加權的 FedAvg；若改成不同大小的分片，必須同時調整平均權重。學習率是 `0.4`。

每個平行項目是一項獨立任務，仍依一般 hcode 規則執行副本共識；三個分片不表示一定會分派到三台不同主機。資料和權重會提供給被指派的 Worker，這個例子沒有隱私保護協定。

從儲存庫根目錄執行，Python 需要 3.10 以上，僅使用標準函式庫：

```powershell
python examples/hcode/federated-training/generate.py
cargo run --manifest-path hivemind-rs/Cargo.toml -p hivemind-bin --bin hivemind-bin -- pipeline check examples/hcode/federated-training/pipeline.json
```

`pipeline.json`、`params/` 及兩個 hcode 檔已提交，可以直接試跑。重新執行產生器會以相同內容重建範例的資料與管線。產生器不依賴亂數版本；所有擾動都由固定的整數公式算出。另存一個只有一輪的版本：

```powershell
python examples/hcode/federated-training/generate.py --rounds 1 --out-dir tmp/federated-one-round
```

四輪的全資料交叉熵依序為：

| 權重狀態 | 平均損失 |
|---|---:|
| 初始零權重 | 1.0986122886681098 |
| 第 1 輪 | 0.6235409589999281 |
| 第 2 輪 | 0.40196651377430265 |
| 第 3 輪 | 0.2873820260556482 |
| 第 4 輪 | 0.2206224401578563 |

固定權重與損失保存在 `expected.json`。`reference.py` 以純量迴圈獨立計算同一演算法，不使用 hcode 執行器、NumPy 或張量套件。測試允許 `1e-12` 的浮點誤差；不同主機的這項參考比較不代表網路副本可以忽略逐位元共識。

```powershell
python examples/hcode/federated-training/reference.py
python -m unittest discover -s examples/hcode/federated-training -v
cargo test --manifest-path hivemind-rs/Cargo.toml -p hivemind-bin --test federated_training
```

Rust 測試實際啟動 CLI，跑完全部八個階段、十六個項目，核對每輪損失嚴格下降與最後的每一個權重。Python 測試驗證產生器可重現、單輪選項、參數合法性與獨立參考值。

若已啟動自己的 Master，並有足夠餘額和可用 Worker，先將 `HIVEMIND_PASSWORD` 環境變數設為帳號密碼，再從同一個 CLI 送出：

```powershell
cargo run --manifest-path hivemind-rs/Cargo.toml -p hivemind-bin --bin hivemind-bin -- pipeline run examples/hcode/federated-training/pipeline.json --api http://127.0.0.1:8082 --username YOUR_USERNAME --task-prefix fedavg-demo --max-total-cpt 150000 --out tmp/fedavg-results.json
```

也可以使用 `--password` 傳入密碼；CLI 不會互動詢問。範例沒有預填 `max_cpt`，所以送出前會在本機量測各項目需要的上限。以三副本計，本機四輪執行量測為 36,406 用量單位、各項目費用上限合計 120,138 CPT；Master 的資源報價可能需要更高上限，請以當次報價與檢查輸出為準。

需要接續同一批任務時，加上 `--resume`，並沿用同一個前綴及結果檔路徑。不要重新產生不同輪數的管線後接續舊任務；內容摘要會拒絕不一致的續跑。

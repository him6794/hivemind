# Hivemind 使用者任務上傳指南

這份說明是給一般使用者看的，目的是讓你知道：

1. 怎麼把任務準備成可上傳的 ZIP。
2. 怎麼把任務送到 Hivemind。
3. 任務程式本身應該怎麼寫，才比較容易成功。

## 1. 先理解任務長什麼樣子

在 Hivemind 裡，一個任務通常就是一個 ZIP 檔。你把程式、資料、設定檔一起打包進去，平台會把這個 ZIP 當成任務來源。

目前系統支援的上傳方式有兩種：

1. CLI 上傳：最適合一般使用者直接傳本機的 ZIP。
2. API 上傳：給自動化腳本或整合用途。

如果你只是想「把一個程式送去跑」，優先用 CLI。

## 2. 任務要怎麼準備

最小可用做法是：

1. 建一個資料夾。
2. 把你的程式碼放進去。
3. 加上你需要的 input 檔、設定檔、依賴說明。
4. 把整個資料夾壓成 `.zip`。

建議保留清楚的入口檔，常見做法是：

- Python：`main.py`
- JavaScript：`index.js`
- 其他語言：放一個容易辨識的啟動腳本

如果你要讓別人一眼看懂，資料夾結構可以像這樣：

```text
my-task/
  main.py
  input.txt
  README.md
```

壓成 ZIP 後，上傳的就是這個 ZIP。

## 3. 程式應該怎麼寫

目前最穩妥的寫法，是把任務程式寫成「單一入口 + 盡量少的外部依賴」。

注意：

- 在目前的開發環境裡，worker 可能是模擬執行模式。
- 你應該先用很小的 Python 任務把「上傳 -> 排隊 -> 查結果」這條流程跑通。
- 如果你要驗證真正的執行效果，請確認部署端已經切到正式 sandbox 設定。

建議原則：

1. 入口檔固定清楚，例如 `main.py`。
2. 盡量只依賴標準函式庫。
3. 從檔案讀輸入，或從程式參數讀輸入。
4. 把結果印到標準輸出。
5. 錯誤資訊印到標準錯誤。

簡單 Python 範例：

```python
import sys


def main():
    if len(sys.argv) < 2:
        print("missing input file", file=sys.stderr)
        sys.exit(1)

    path = sys.argv[1]
    with open(path, "r", encoding="utf-8") as f:
        content = f.read().strip()

    print(content.upper())


if __name__ == "__main__":
    main()
```

這種寫法的好處是：

- 容易打包。
- 容易除錯。
- 容易重現結果。

## 4. 上傳任務的方法

### 方法 A: CLI 上傳

如果你本機已經有 ZIP，這是最直接的方法。

```bash
hivemind submit ./my-task.zip \
  --api http://localhost:8082 \
  --username <username> \
  --password <password> \
  --task-id <task-id> \
  --memory-gb 2 \
  --host-count 1 \
  --max-cpt 100
```

重點說明：

- `task-id` 是你自己給任務取的名稱。
- `task-id` 只能用安全檔名字元，不能包含 `../` 這類路徑跳脫內容。
- `zip` 不能是空檔。
- `max-cpt` 是你願意接受的最高價格，超過就會被擋下來。

### 方法 B: API 上傳

如果你要寫腳本或整合其他工具，可以呼叫：

`POST /api/tasks/upload`

這個 API 會帶：

- `task_id`
- `memory_gb`
- `cpu_score`
- `gpu_score`
- `gpu_memory_gb`
- `storage_gb`
- `host_count`
- `max_cpt`
- `file`（ZIP 本體）

### 方法 C: 先填 URL / torrent

如果你的任務來源不是本機 ZIP，也可以走 `POST /api/tasks`，傳 `torrent` 或 `zip_path`。

不過對一般使用者來說，最方便的還是直接提交 ZIP。

如果你是用 Web 介面，現在的頁面欄位偏向填「master 主機上看得到的 ZIP 路徑」；如果你手上只有本機檔案，CLI 會更直接。

## 5. 上傳後怎麼看結果

任務送出後，你可以用這些方式查看：

1. 任務列表：看狀態是 `PENDING`、`RUNNING`、`COMPLETED` 還是 `FAILED`。
2. 任務日誌：看執行過程。
3. 任務結果：看結果檔或結果 torrent。

CLI 也支援：

```bash
hivemind status <task-id> \
  --api http://localhost:8082 \
  --username <username> \
  --password <password>

hivemind result <task-id> \
  --api http://localhost:8082 \
  --username <username> \
  --password <password>
```

## 6. 常見失敗原因

1. ZIP 不是有效檔案，或是空檔。
2. `task-id` 不合法。
3. `max_cpt` 比系統算出的報價還低。
4. 程式缺少入口檔。
5. 依賴太多，執行環境沒有安裝。

## 7. 實務建議

如果你是第一次用，建議先做一個最小任務：

1. 只放一個 `main.py`。
2. 讀一個簡單輸入檔。
3. 把結果印出來。
4. 確認可以上傳、可以看到狀態、可以查到結果。

等這條流程通了，再慢慢增加依賴和複雜度。

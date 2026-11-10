# hcode 教學：從 JSON 輸入到可結算的任務

hcode 是 Hivemind 的 CPU 任務語言。提交一段 source 和 JSON 資料，Nodepool 會安排 Worker 副本執行、驗證共識與用量，再回傳結果並結算。它適合小型資料轉換、加總、條件判斷與批次計算，不是 Python、JavaScript，也不是可以執行任意程式的容器。

本教學使用目前的新任務契約 `hcode`。完整參考見 [hcode Runtime](HCODE_RUNTIME.md)，可修改的範例見 [templates/hcode](../templates/hcode/)。

## 1. 執行模型

1. Master 把 `task_source` 與 JSON input 交給 Nodepool；Master 不負責確認計費或執行結果可信。
2. Worker 以封閉直譯器進行詞法分析、語法分析與求值；JSON input 綁定為 `input`。
3. 每個副本使用 Nodepool 分配的執行額度，產生最終值、`print` 輸出和用量 receipt。
4. Nodepool 驗證指派身分、結果綁定、共識與用量證據，決定任務結果與實際費用。

直譯器沒有檔案系統、網路、環境變數、import、subprocess、eval、DLL 或 native API。CPU hcode 不需要 Docker、WSL 或另一個 shell，也不會在失敗時改用主機直接執行。任務必須是確定性、無外部副作用的計算；Worker 共識不是密碼學正確性證明。

最終值除頂層字串外，以緊湊 JSON 表示，dict keys 有固定排序；頂層字串以原始 UTF-8 輸出。`print` 是額外的文字輸出，不等同最終值。

## 2. 第一個程式：加總三個數字

將下列內容存為 UTF-8 的 `sum.hdsl`：

```text
let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;
```

輸入資料：

```json
{"values": [10, 20, 30]}
```

最後的 `total;` 讓程式以加總值作為結果。這個程式沒有 `print`，因此沒有額外的 stdout；最終值為：

```text
60
```

在目前固定的 hcode cost model 下，此範例每個成功副本使用 **25 usage units，即 25 CPT 的副本用量**。這不是整個任務的總費用；副本加總與平台費見第 5 節。執行回歸測試同時核對這段 source、input、結果與用量。

## 3. 語法入門

### 變數、函式與迴圈

`let` 可宣告或重新綁定變數。更新 `total` 必須寫 `let total = total + value;`，不能寫裸的 `total = total + value;`。一般 statement 以分號結尾。

```text
fn add(a, b) {
  return a + b;
}
add(get(input, "left"), get(input, "right"));
```

函式是頂層具名函式，`return` 提早回傳值。`for item in values { ... }` 只遍歷 list，不遍歷 dict、string。需要整數序列時用 `range(...)` 產生 list（見下方 builtins）。

`if` 是 expression，必須有 `else`；每個 branch 是單一 expression，不是任意 statements block：

```text
let count = get(input, "count");
if count > 0 { count } else { 0 };
```

### 型別與運算

CPU hcode 支援 signed `i64` 整數、有限的 `f64` 浮點數、boolean、string、list、string-keyed dict、`null`，以及由 tensor builtins 建立的 tensor。

| 寫法 | 說明 |
| --- | --- |
| `1.5`、`1e3` | 浮點數常量；`-0.0` 會正規化為 `0.0`，`1e400` 這類超出範圍的常量會被拒絕 |
| `+ - * /` | 整數與浮點算術；整數超出 i64 範圍、除以零，或任何浮點運算產生 NaN／無限大，都會失敗。tensor 逐元素運算要求同形狀，或另一側是純量 |
| `%` | 整數取餘（CPU hcode）；結果符號與被除數相同，除以零失敗 |
| `& \| ^ ~ << >>` | 位元運算（CPU hcode）；移位數須在 0 到 63，`>>` 為算術右移 |
| `== != < <= > >=` | 比較運算，運算元仍須符合型別要求 |
| `and or not` | Boolean 運算；不是 JavaScript 的 `&&`／`||` |
| `values[index]` | List 或 string 索引可為負數；越界失敗 |
| `row["key"]` | Dict 的 string key 存取 |
| `values[index] = value;` | 可更新 simple variable target 的 list／dict 元素；不是任意巢狀 lvalue |
| `print(expr);` | 額外輸出；受 output cap 約束且消耗用量 |

### CPU builtins

| Builtin | 行為 |
| --- | --- |
| `len(value)` | List 元素數、dict entries 數或 string 的 **UTF-8 byte 數** |
| `get(target, key)` | Dict＋string key，或 list＋非負 integer index；缺 key／越界回 `null`；不支援 string target |
| `contains(target, value)` | Dict key 存在、string substring 或 list equality membership |
| `abs(n)`、`min(a, b)`、`max(a, b)` | 整數絕對值、最小值與最大值（CPU hcode）；`abs(i64::MIN)` 會失敗 |
| `range(stop)`、`range(start, stop)`、`range(start, stop, step)` | 產生 int list（CPU hcode）；每個元素計一次用量，超過 collection 上限會失敗；step 不可為 0 |
| `wrapping_add`、`wrapping_sub`、`wrapping_mul` | 整數運算保留低 64 位元，不因溢位失敗（CPU hcode） |
| `tensor(list)`、`zeros(shape, "i64")`、`zeros(shape, "f64")` | 建立 tensor（CPU hcode）。巢狀 list 必須是矩形；含任一 float 時為 `f64`；元素數超過 collection 上限會失敗 |
| `shape(t)`、`reshape(t, shape)`、`transpose(t)`、`tolist(t)`、`cast(t, "f64")` | 查詢或改變形狀、轉成巢狀 list、改變 dtype。`transpose` 只支援 rank 2 |
| `sum(t)`、`dot(a, b)`、`matmul(a, b)` | 總和、向量內積，以及矩陣乘法（矩陣×矩陣、矩陣×向量、向量×矩陣）。每次乘加計一次用量 |
| `relu(t)`、`gelu(t)`、`softmax(t)` | 激活函式。`gelu` 每元素計 24 單位；`softmax` 沿最後一維計算，每元素計 20 單位 |
| `freivalds_check(a, b, c, seed, rounds)` | 驗證 `a * b == c`（i64 矩陣，元素在 i32 範圍內，`rounds` 為 1 到 64），不必重算乘積。正確乘積必定通過。理論上的 `2^-rounds` 界需要獨立均勻、與固定聲稱無關的隨機向量；本函式使用固定 seed 的 splitmix64，沒有已證明的對抗性機率保證，不能取代 Nodepool 共識驗證 |
| `float(x)`、`int(x)` | 整數與浮點數互轉（CPU hcode）；`int` 向零截斷，超出 int64 範圍會失敗 |
| `sqrt`、`floor`、`ceil`、`round` | 平方根與取整（CPU hcode）；負數開根號會失敗 |
| `exp`、`ln`、`pow`、`sin`、`cos`、`tanh` | 超越函式（CPU hcode）；使用固定的軟體級數而非主機 libm，每次呼叫另計 16 單位用量。`sin`、`cos` 接受 `\|x\|` 至 1000000；`ln` 需要正數 |

`len(string)` 計 bytes，但 `string[index]` 按 characters；不可用前者直接推算後者的有效索引。上表的 builtin 名稱在 hcode 中是保留字，`fn` 不能使用同名。

### 不支援的語法

- `while`、`break`、`continue`、member-dot access（例如 `input.values`）。
- Source comments：不要在 hcode source 加 `//`、`#` 或 `/* ... */`。
- 任意 host function、第三方套件、GPU kernel 或使用者 executable。

**中文資料放 JSON input，不要直接寫在 source string literals。** Source lexer 的字串是 byte-wise decoding，無法保證 non-ASCII literal 原樣保留，也不接受 `\uXXXX`。可用的 source escapes 是 `\"`、`\\`、`\n`、`\r`、`\t`。JSON input 與輸出支援 UTF-8；例如把 `{"message":"你好"}` 當 input，source 使用 `get(input, "message");`。

## 4. 提交資料的格式

CPU 新任務固定使用 `"runtime": "hcode"`：

```json
{
  "task_id": "hcode-sum-example",
  "runtime": "hcode",
  "task_source": "let total = 0;\nfor value in get(input, \"values\") {\n  let total = total + value;\n}\ntotal;",
  "torrent": "{\"values\": [10, 20, 30]}",
  "max_cpt": 1000
}
```

`POST /api/tasks` 需要 Nodepool 簽發的 bearer token，請求送到自己的 **Master HTTP API**，不是官網或瀏覽器直連 Nodepool gRPC。

- `task_source` 是 source text，不是檔案路徑或下載 URL。
- `torrent` 是歷史沿用的欄位名稱；hcode 在此放 **JSON 字串**，不是 JSON object，也不是 torrent、ZIP 或 executable。
- Source 與 input 都必須非空；不需要資料時，可用 JSON 字串 `"null"` 作為 `torrent`。
- JSON number 必須能表示為 signed i64 整數；不要使用小數、超大整數或指數形式假設會被接受。
- Task ID 在同一帳號的工作流程中應唯一；重送前先確認原請求是否已成功，避免重複工作。
- CPU／GPU 分數及記憶體等 requirements 是排程篩選條件；不是更換語言或取得 GPU execution 的方式。

## 5. max_cpt、所有副本與平台費

**新任務的 `max_cpt` 是整個任務的總收費上限，包含所有副本與 10% 平台費，不是每個 Worker 各自的上限。** 預設 3 個不同 Worker 副本，2 個一致結果形成 quorum；設定可調整副本數，但需符合 Nodepool policy。

設任務上限為 `M`，副本數為 `R`，每副本額度 `B` 是滿足下式的最大正整數：

```text
R × B + floor(R × B / 10) ≤ M
```

平台費以 aggregate accepted usage 的 10% **向下取整**，不是每副本分別進位。若預算不足以分配正整數額度，任務會被拒絕。

本教學 `M = 1000`、`R = 3`：

| 項目 | CPT |
| --- | ---: |
| 每個副本的執行額度 | 303 |
| 最多保留的副本用量 | 909 |
| 對應平台費 | 90 |
| 派發前的總保留額 | 999 |

若三個副本都成功且各產生 25 單位有效用量，沒有其他有效用量，則實際用量是 `25 × 3 = 75 CPT`，平台費 `floor(75 / 10) = 7 CPT`，**總收費為 82 CPT**，未使用的保留額退還。

- 一個 executed operation 對應一個 replica usage CPT；不是一行 source 等於一個 CPT，也不是按牆鐘時間收費。Statement、expression、argument、call overhead、loop iteration 與 print 都可能計量。
- 預算耗盡的副本無法貢獻成功結果，但在耗盡前的有效用量仍可能結算。未達 quorum 不代表免費；有效的不同結果副本也可能獲付款。
- Nodepool 決定哪些 receipt 有效；Worker 自報的 counters 不會直接成為費用。
- 成功結果出現時，其他副本可能仍在停止；`COMPLETED` 不等於 `billing_settled=true`。
- Observe 模式只記錄 shadow evidence，不保留或結算 CPT。自動付費重試目前停用。
- 更早的 per-replica `max_cpt` 歷史帳務不會套用新 task-wide cap 重新計算。

## 6. 三種提交方式

### Master UI

登入自己的 Master UI，切到 **Submit／提交**，填入 source、input 和 **Task charge cap／任務收費上限**。上方語言選單可在 English 與繁體中文間切換；預設英文，偏好分別保存在 Master／Worker UI 的本機瀏覽器儲存空間。

此範例使用 hcode 的 `get(input, "values")`，不要寫 JavaScript 形式的 `input.values`。Task 的最終輸出從 **Log／日誌** 讀取；不是舊式 torrent result。

### PowerShell：經 Master HTTP API

下列指令供使用者在互動式 PowerShell 執行。先把第 2 節 source 存成 `sum.hdsl`，再登入並提交；範例只顯示本機地址與提示輸入的帳密，不把 token 寫入檔案。

```powershell
$master = "http://127.0.0.1:8082"
$credential = Get-Credential -Message "Hivemind account"
$loginBody = @{
    username = $credential.UserName
    password = $credential.GetNetworkCredential().Password
} | ConvertTo-Json -Compress
$login = Invoke-RestMethod -Method Post -Uri "$master/api/login" `
    -ContentType "application/json" -Body $loginBody
if (-not $login.token) { throw "Login did not return a token" }
$headers = @{ Authorization = "Bearer $($login.token)" }

$taskId = "hcode-sum-" + [Guid]::NewGuid().ToString("N")
$payload = @{
    task_id = $taskId
    runtime = "hcode"
    task_source = Get-Content -LiteralPath "./sum.hdsl" -Raw -Encoding UTF8
    torrent = '{"values": [10, 20, 30]}'
    max_cpt = 1000
} | ConvertTo-Json -Depth 2 -Compress
$created = Invoke-RestMethod -Method Post -Uri "$master/api/tasks" `
    -Headers $headers -ContentType "application/json; charset=utf-8" `
    -Body ([Text.Encoding]::UTF8.GetBytes($payload))
if (-not $created.success) { throw "Task submission was rejected" }

$list = Invoke-RestMethod -Method Get -Uri "$master/api/tasks" -Headers $headers
if (-not $list.success) { throw "Task listing was rejected" }
$list.tasks | Where-Object { $_.task_id -eq $taskId }
Invoke-RestMethod -Method Get -Uri "$master/api/tasks/$taskId/log" -Headers $headers
```

最後兩個 GET 是一次查詢，不代表已等待完成。稍後可重查 task status 與 log；有界輪詢範例見[手冊 §9](HIVEMIND_MANUAL.zh-TW.md#api)。平面 payload 使用 `ConvertTo-Json -Depth 2`，避免 Windows PowerShell 5.1 對含換行 source 使用過高 depth 時的序列化問題。

### CLI

全功能 `hivemind-bin` 的 `submit` 子命令會讀取 source 檔案並提交 hcode。以下使用 POSIX shell 引號；`${HIVEMIND_USER}` 與 `${HIVEMIND_PASSWORD}` 由操作者先設定，範例沒有真實帳密：

```bash
hivemind-bin submit ./sum.hdsl \
  --api http://127.0.0.1:8082 \
  --username "$HIVEMIND_USER" --password "$HIVEMIND_PASSWORD" \
  --task-id hcode-sum-cli \
  --input '{"values": [10, 20, 30]}' \
  --max-cpt 1000
```

送出前可以先在本機試跑。`check` 以副本相同的限制執行一次，不需要帳號、不連線、不扣款，並算出這筆輸入需要的任務總上限：

```bash
hivemind-bin check ./sum.hdsl --input '{"values": [10, 20, 30]}'
```

```text
Output:
60

Usage per replica: 25 units
Replicas: 3 (the network default; change it with --replicas)
Reserved usage: 75 CPT
Platform fee: 7 CPT
Minimum --max-cpt: 82 CPT

This is an estimate from a run on this machine (interpreter semantics d82fb2679d7f). Nodepool settles on what the Workers report.
```

`Minimum --max-cpt` 是讓每個副本都能跑完的最小總上限，也是三個副本全部執行並被接受時的收費；低於它，副本會因 `budget_exhausted` 失敗。它是預估：本機與 Worker 的直譯器版本不同時，用量可能不同，輸出裡的 interpreter semantics 摘要可用來比對。`check` 也接受 `--artifact name=path.safetensors`、`--replicas`（2 到 7）、`--max-usage` 與 `--timeout-secs`（本機試跑自己的上限）。函式在本機就失敗時，語法錯誤會附行與欄，其他錯誤只有代碼與原因。

`submit --max-cpt auto` 會先做同樣的本機試跑，再以預估的總上限送出（若資源報價更高則取報價）；函式在本機失敗時不會送出任何東西。需要 `--input`，並可搭配 `--replicas`。

`submit` 只接受 `--input <json>`，不接受 `--input-file`。多筆輸入請改用 `batch`：

```bash
hivemind-bin batch ./sum.hdsl \
  --api http://127.0.0.1:8082 \
  --username "$HIVEMIND_USER" \
  --inputs ./inputs.jsonl \
  --max-total-cpt 500 \
  --out ./sum-results.csv
```

`inputs.jsonl` 每行一個 JSON 值，空行會被略過，最多 1000 筆。`--max-total-cpt` 是必填的總預算上限：各筆任務的上限相加若超過它，命令直接拒絕，不會提交任何任務。

`batch` 送出前先對每一筆輸入在本機試跑一次（與 `check` 相同，不需要網路），所以下面三件事在花錢之前就已確定：

- **會失敗的輸入。** 語法錯誤、執行錯誤、超過本機試跑上限的輸入，預設會讓整批都不送出；命令列出前 10 筆的序號與原因。加上 `--skip-invalid` 則只送出能跑的輸入，被跳過的在結果 CSV 標為 `PRECHECK_FAILED` 並附原因。
- **每筆的上限。** 預設是該輸入自己的精確需求（副本數 × 用量，加 10% 平台費），但不低於資源報價（報價只計算資源預留，不含程式執行的用量）。總額是各筆上限相加，再與 `--max-total-cpt` 比較。
- **固定上限。** 指定 `--max-cpt-per-task` 時，每筆都用這個上限，需要超過它的輸入同樣在預檢被擋下，不會等到 Worker 以 `budget_exhausted` 失敗。

`--replicas`（2 到 7）選擇副本數，並隨任務送給 Nodepool；沒有指定時以網路預設的 3 個副本估算。`--no-precheck` 關閉預檢，此時每筆上限預設為 100 CPT（不低於資源報價），超過上限的任務會以 `budget_exhausted` 失敗，依目前的計費規則，未達 quorum 的任務不收費。

提交速度預設為每分鐘 50 筆，低於 Master 的每分鐘提交上限（預設 60）；遇到 429 會等候視窗重置後再試。全部任務結束或超過 `--timeout-secs`（預設 3600）後，結果寫入 CSV，欄位為 `index,task_id,status,output,billed_cpt,message,input`。只有全部任務都 `COMPLETED` 時命令才以成功結束；被跳過的輸入不算完成，所以使用 `--skip-invalid` 時命令仍以失敗結束，結果檔會標出是哪幾筆。

批次開始時會在結果檔旁寫一個狀態檔（`<--out>.state.json`），記錄函式、每筆輸入的摘要、資源與副本設定，以及每個任務的上限。命令中斷後，用相同的 `--out`、`--task-prefix` 與檔案，加上 `--resume` 重新執行：它先向網路查出已存在的任務，只送出缺少的，上限沿用狀態檔的數字，不會重新報價，也不會重複送出或重複扣款。

函式、任何一筆輸入、資源或副本設定與狀態檔不同時，`--resume` 會拒絕，因為任務 ID 由前綴與序號決定，續跑會把舊的任務 ID 配到不同的資料上。沒有 `--resume` 時，已有狀態檔的批次不會被覆寫；要重跑同一批資料，請換一個 `--task-prefix` 或 `--out`。

密碼可以用 `HIVEMIND_PASSWORD` 環境變數提供，不必出現在命令列。CLI 的 `--password` 仍可能被本機程序檢視工具看見；不要在共享機器的命令列硬寫正式帳密，也不要把命令輸出、token 或私密 input 貼到公開紀錄。原生 Master／Worker UI 執行檔不是這個 CLI。

## 7. 限制與錯誤排查

| 邊界 | 目前限制 |
| --- | --- |
| Source UTF-8 bytes | 65,536 bytes（64 KiB） |
| JSON input 字串 | 1,048,576 bytes（1 MiB） |
| User function call depth | 64 |
| Interpreter output | 1,048,576 bytes |
| 單一 value | 1,048,576 canonical logical JSON bytes |
| List／dict 直接元素數 | 100,000 |
| Value nesting depth | 64 |
| 累計 materialization | 16,777,216 logical bytes（16 MiB） |
| Consensus result envelope | 262,144 bytes（256 KiB），另受 operator 設定約束 |
| Round timeout | 預設 120 秒，可由 operator policy 覆寫 |

Interpreter output cap 不代表 transport 能裝下同樣大小的結果：envelope 還包含 metadata，實際 output 必須小於 wire cap。Logical memory accounting 也不等同 OS RSS 限額。

hcode 沒有另一個固定的 operation／loop work ceiling 取代預算；每副本仍有額度、deadline 與上述結構限制。

| 現象／code | 先檢查 |
| --- | --- |
| `parse_error` | 分號、`if...else`、裸 assignment、註解或 member-dot access |
| `type_error` | JSON number 是否為 i64、運算元型別、list／dict 的 builtin 用法 |
| `index_error` | Index 是否越界；需要缺值回 null 時可考慮 `get` |
| `integer_arithmetic_overflow` | 中間運算是否超出 signed i64 範圍 |
| `budget_exhausted` | 實際每副本額度，不只看 task-wide `max_cpt` |
| 無可用 Worker／未達 quorum | Worker runtime identity、可用副本數、資源條件及 Nodepool 診斷 |
| 輸出／value／deadline 相關失敗 | 對照當前版本的 receipt code 與上述 bounds，縮小資料或工作量 |

大小及 budget admission 通過不代表 DSL 或 input 型別一定有效；部分錯誤到 Worker 執行時才會被發現。只採用 Nodepool 驗證的最終結果與帳務，原始 Worker 訊息是診斷資料。

## 8. 舊版遷移與 GPU

`managed-function-v1` 已退役，**不是 hcode 的 alias**；新提交送舊名字會被拒絕。GPU 同樣由 `managed-function-gpu-v1` 改為 `hcode-gpu`，相關 request/result/capability、cost、billing 與 semantics 身分一起改名。Operation costs、10% 平台費、quorum 與結算規則不因改名而改變。

升級順序：

1. 停止接受新提交，以舊版 Nodepool 完成／取消並結清舊 CPU 回合，包含 terminal task 的 stop-pending round 和尚未釋放的 Worker reservation。
2. 以舊版 drain／cancel 帶有 typed manifest 的未完成舊 GPU 任務。
3. 停止舊 Nodepool，協調升級 Nodepool、Master、Worker，再恢復提交；不支援新舊版本混跑並透明接續舊回合。

Migration 發現上述未完成工作就拒絕啟動並 rollback。通過後會更新舊 CPU task runtime，以及仍為 `held` 的舊 billing/cost metadata；不重寫既有 request/result/attempt/certificate evidence，也不修改 `settled`／`released` 歷史帳務。GPU typed request/result/digest 不會被就地重打成新契約；缺 manifest 的舊 active GPU row 仍走既有 quarantine。不要手動改資料庫來繞過檢查。

Frozen `managed-function-v0` 只供歷史讀取，語義、digest、receipt 和帳務保持不變，不接受新工作或續跑。

`hcode-gpu` 是**另一份 typed GPU 契約**，使用 operator-owned 執行資產與裝置綁定，不是把本教學 payload 的 runtime 換個名字就能執行，也不允許使用者上傳任意 kernel。CPU hcode source 的型別、builtins 與計量說明不能直接當作 GPU API。

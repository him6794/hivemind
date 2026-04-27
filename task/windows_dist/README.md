# Windows 分發（API 端 / 用戶端）

這份資料夾提供 **Windows** 用的分發整理方式：用一個 `pack.ps1` 直接在本 repo 內產生兩個可複製的資料夾：

- `out\api_server\`：Flask API + 前端頁面（index.html）
- `out\worker_client\`：worker 用戶端（呼叫 API 拿任務 + 回報進度），需要 `prime_sieve.dll`

## 1) 產生分發資料夾
在 repo 根目錄（或任何位置）用 PowerShell 執行：

```powershell
powershell -ExecutionPolicy Bypass -File .\task\windows_dist\pack.ps1
```

完成後會得到：

- `task\windows_dist\out\api_server\`
- `task\windows_dist\out\worker_client\`

把這兩個資料夾分別複製到你的 API 主機與 40 台用戶端主機即可。

## 2) API 端（server）要安裝什麼、怎麼跑
到 `api_server` 目錄：

```powershell
python -m venv .venv
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\python.exe api.py
```

預設會在 `http://0.0.0.0:5001` 監聽。

## 3) 用戶端（worker）要安裝什麼、怎麼跑
到 `worker_client` 目錄：

```powershell
python -m venv .venv
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\python.exe main.py http://<API_SERVER_IP>:5001
```

### 重要：prime_sieve.dll
`worker_client` **必須**放入 `prime_sieve.dll`（以及它需要的相依 DLL，例如 MinGW 的 `libgomp-1.dll` 若有）。

- 建議做法：把 `prime_sieve.dll` 與相依 DLL 都放在 `main.py` 同一層。
- 若你目前 repo 的 `task\prime_sieve.dll` 存在，`pack.ps1` 會自動複製進去。

## 4) 常見問題
- 若 worker 一執行就說找不到 DLL：通常是缺 `prime_sieve.dll` 或缺 OpenMP runtime（如 `libgomp-1.dll`）。
- 若要更換 API 位址：直接在執行命令後面傳參數（如上）。

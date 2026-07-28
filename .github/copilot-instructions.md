# HiveMind：AI coding agent 指引（針對本 repo）

## Big picture（先懂控制面/資料面）
- **NodePool（控制面）**：`node_pool/`，gRPC server 入口 `node_pool/node_pool_server.py`，管理使用者、worker 註冊/狀態、任務上傳/停止/結果下載；狀態主要落在 **Redis**（節點 `node:<id>`、任務 `task:<id>`），任務 ZIP/結果 ZIP 由 `TASK_STORAGE_PATH` 寫檔（參考 `node_pool/master_node_service.py`）。
- **Worker（資料面）**：`worker/src/hivemind_worker/`，同時是
  - gRPC **server**：提供 `WorkerNodeService.ExecuteTask`（`grpc_servicer.py`）接收任務
  - gRPC **client**：呼叫 NodePool 的 `UserService/NodeManagerService/MasterNodeService/WorkerNodeService` 回報 output/result/usage（`grpc_client.py`）
  - 任務實際執行與打包集中在 `task_executor.py: execute_task()`（Docker 優先，fallback venv）。
- **Master UI**：`master/hivemind_master/src/hivemind_master/master_node.py`（Flask UI + NodePool gRPC client），負責上傳 zip、查任務、停任務、下載結果。

## gRPC / proto（修改契約要同步三邊）
- 單一 proto：`node_pool/nodepool.proto`，產物 `nodepool_pb2*.py` 在 **NodePool / Worker / Master** 都各自有一份。
- VS Code tasks（Windows）可產生 pb：`protoc-generate-node-pool` / `protoc-generate-worker` / `protoc-generate-master`。
- 注意「語意陷阱」：`WorkerNodeService` 在 **worker 與 nodepool 兩側都實作**但用途不同（參考 `docs/developer-architecture.md`、`docs/rpc-contract-notes.md`）。

## 常用開發流程（本 repo 的慣例）
- 啟動順序（最小可跑）：
  1) Redis
  2) NodePool gRPC：`node_pool/node_pool_server.py`（預設 50051）
  3) Worker：`worker/src/hivemind_worker/__main__.py` 或 `worker/main.py`（會連 `NODEPOOL_ADDRESS`）
  4) Master UI（可選）：`master_node.py`（預設 5002）
  
- Worker Windows 編譯（Nuitka）：`worker/build_worker.ps1`（見 `worker/BUILD.md`）
  - 編譯前會把 `node_pool/nodepool_pb2*.py` 複製到 worker。
  - `worker/runtime/` 可能包含 **bundled Python runtime**（B1）供無系統 Python 的 venv fallback（同樣由 build script 複製到 dist）。

## 任務 ZIP 格式與執行規則（很常踩坑）
- 任務 ZIP 建議包含：`main.py`（或 `app.py/run.py/start.py`）+ 可選 `requirements.txt`（見 `master/.../templates_master/master_upload.html` 與 `task_executor._find_executable_script()`）。
- Worker 執行路徑：解壓到 temp workspace（`task_executor._safe_extract_zip()` 有 ZipSlip 防護）。
- Docker 不可用時走 venv：`task_executor._create_task_venv()`（Windows 優先 `runtime\Scripts\virtualenv.exe`）。

## 設定與環境變數（先查設定檔再改 code）
- Worker：`worker/src/hivemind_worker/config.py`
  - 特別是 `NODEPOOL_ADDRESS`（預設值可能與 docs 不一致，遇到連線問題先看這裡與環境變數覆寫）。
- Master：`master_node.py` 使用 `GRPC_SERVER_ADDRESS`。

## 測試位置與寫法（以 worker 為主）
- Worker 單元測試在：`worker/src/hivemind_worker/tests/`（pytest）。
- 若修改任務執行/venv/python resolution，請同步更新對應測試（例如 `test_task_executor_uses_bundled_python_and_cleans.py`）。

## 變更建議（避免踩雷）
- 任何 proto 變更：務必更新三邊生成檔 + 相關呼叫端（node_pool/worker/master），否則會出現 runtime gRPC 序列化/欄位缺失問題。
- 請先讀 `docs/developer-runbook.md`（啟動與故障定位）與 `docs/rpc-contract-notes.md`（已知語意陷阱）。

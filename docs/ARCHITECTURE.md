# Hivemind — Architecture Overview

- nodepool: 調度與資源管理核心，負責註冊/排程/分配任務。
- master: 使用者上傳任務（torrent），向 nodepool 請求執行並查詢結果。
- worker: 執行任務，定期回報狀態並上傳輸出或結果 torrent。
- executor (Rust): 在 worker 內以 sandboxed 方式執行 Python 任務，限制資源。
- frontend: 提供管理介面（選用）。
- infra: redis/kafka/postgres etc.，由 `docker-compose.yml` 管理。

通訊：gRPC 使用 `hivemind.proto`，任務檔案透過 torrent/magnet 或外部儲存。
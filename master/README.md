# Master Service

## 職責
- 任務提交/查詢 API、工作者註冊/心跳
- 調度與重試策略
- 事件發佈（參見 docs/events/events.md）

## 環境
- DATABASE_URL
- REDIS_URL
- NATS_URL
- MASTER_CONFIG: 設定檔路徑

## 開發建議
- 以 docs/apis/master.openapi.yaml 為契約優先
- 先完成 /health, /tasks(POST), /workers(POST), /workers/{id}/heartbeat
- 調度可先採用最簡先入先出 + 標籤匹配

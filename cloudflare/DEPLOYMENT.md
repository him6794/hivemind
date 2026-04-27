# Hivemind Update Service 部署指南

## 1. 前置準備

### 安裝 Wrangler CLI
```bash
npm install -g wrangler
```

### 登入 Cloudflare
```bash
wrangler auth login
```

## 2. 建立必要資源

### 建立 KV 命名空間
```bash
wrangler kv:namespace create "VERSIONS" --env production
```
記錄返回的 KV namespace ID，填入 `wrangler.toml` 的 `id` 欄位

### 建立 R2 儲存桶
```bash
wrangler r2 bucket create hivemind-binaries
```

## 3. 設定環境變數

編輯 `wrangler.toml`：
- `ADMIN_PASSWORD`: 設定管理面板密碼
- `PUBLIC_R2_BASE_URL`: 設定你的 R2 公開訪問域名
- 更新 KV namespace ID

## 4. 部署
```bash
wrangler deploy --env production
```

## 5. 設定自定義域名 (可選)

在 Cloudflare Dashboard 中：
1. 進入 Workers & Pages
2. 選擇你的 Worker
3. 點擊 "Custom domains"
4. 添加你的域名

## 6. API 端點說明

部署完成後，你會有以下端點：

### 公開端點 (無需認證)
- `GET /worker/manifest` - 獲取 worker 版本資訊
- `GET /master/manifest` - 獲取 master 版本資訊

### 管理端點 (需要認證)
- `GET /admin` - 管理面板
- `POST /api/upload/{channel}` - 上傳新版本檔案
- `POST /api/version/{channel}` - 更新版本資訊

## 7. 使用流程

1. **登入管理面板**: 訪問 `https://your-domain.com/admin`
2. **設定 Base URL**: 在系統設定中設定 R2 的公開訪問網址
3. **上傳版本**: 使用 Artifact 上傳功能上傳新版本
4. **客戶端更新**: 客戶端定期檢查 manifest 端點獲取最新版本

## 8. 客戶端整合範例

```python
import requests

# 檢查更新
def check_for_updates(channel='worker'):
    response = requests.get(f'https://your-domain.com/{channel}/manifest')
    manifest = response.json()
    latest_version = manifest.get('latest')
    
    if latest_version:
        # 取得對應平台的下載連結
        versions = manifest.get('versions', {})
        if latest_version in versions:
            artifacts = versions[latest_version].get('artifacts', [])
            for artifact in artifacts:
                if artifact['os'] == 'windows' and artifact['arch'] == 'x86_64':
                    download_url = artifact['download_url']
                    # 下載並安裝更新
                    break
```

## 9. 安全注意事項

- 確保 `ADMIN_PASSWORD` 足夠複雜
- 考慮啟用 Cloudflare Access 保護管理端點
- 定期檢查 Worker 使用量和日誌
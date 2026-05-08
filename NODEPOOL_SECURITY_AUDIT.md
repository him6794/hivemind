# NodePool 安全審查報告

## 執行摘要

本報告對 Hivemind NodePool 服務進行了全面的安全審查。NodePool 是一個關鍵的協調服務，負責任務分配、Worker 管理、用戶認證和計費結算。

**審查日期**: 2026/4/28  
**審查範圍**: services/nodepool/cmd/server/main.go (2149 行)  
**發現問題**: 8 個安全問題（2 高危、4 中危、2 低危）

---

## 安全問題清單

### 🔴 HIGH-01: 文件上傳無大小限制驗證（DoS 風險）

**位置**: Line 1822 - `io.ReadAll(file)`  
**嚴重程度**: 高危  
**CVSS 評分**: 7.5

**問題描述**:
```go
fileData, err := io.ReadAll(file)  // 無限制讀取文件到內存
```

雖然 `ParseMultipartForm(100 * 1024 * 1024)` 設置了 100MB 限制，但 `io.ReadAll` 仍會將整個文件加載到內存中，可能導致：
- 內存耗盡（OOM）
- 服務拒絕攻擊
- 多個並發上傳可能使服務崩潰

**影響範圍**:
- `/api/create-torrent` 端點
- 所有認證用戶都可觸發

**修復建議**:
```go
// 使用 LimitReader 限制讀取大小
maxSize := int64(100 * 1024 * 1024) // 100MB
limitedReader := io.LimitReader(file, maxSize+1)
fileData, err := io.ReadAll(limitedReader)
if err != nil {
    w.WriteHeader(http.StatusInternalServerError)
    _ = json.NewEncoder(w).Encode(map[string]any{
        "success": false, 
        "status_message": "failed to read file"
    })
    return
}
if int64(len(fileData)) > maxSize {
    w.WriteHeader(http.StatusRequestEntityTooLarge)
    _ = json.NewEncoder(w).Encode(map[string]any{
        "success": false, 
        "status_message": "file too large (max 100MB)"
    })
    return
}
```

**優先級**: P0 - 立即修復

---

### 🔴 HIGH-02: JWT Secret 硬編碼（認證繞過風險）

**位置**: Line 2089-2092  
**嚴重程度**: 高危  
**CVSS 評分**: 9.1

**問題描述**:
```go
jwtSecret := os.Getenv("NODEPOOL_JWT_SECRET")
if jwtSecret == "" {
    jwtSecret = "dev-secret-change-me"  // 硬編碼的默認密鑰
}
```

如果環境變量未設置，系統使用硬編碼的弱密鑰，攻擊者可以：
- 偽造任意用戶的 JWT Token
- 繞過身份驗證
- 訪問所有用戶數據
- 執行未授權操作

**影響範圍**:
- 所有需要認證的 API 端點
- 用戶餘額查詢
- 任務管理
- Worker 管理

**修復建議**:
```go
jwtSecret := os.Getenv("NODEPOOL_JWT_SECRET")
if jwtSecret == "" {
    log.Fatal("NODEPOOL_JWT_SECRET environment variable is required")
}
if len(jwtSecret) < 32 {
    log.Fatal("NODEPOOL_JWT_SECRET must be at least 32 characters")
}
```

**優先級**: P0 - 立即修復

---

### 🟡 MED-01: SQL 注入風險（時間戳參數）

**位置**: Line 1708  
**嚴重程度**: 中危  
**CVSS 評分**: 6.5

**問題描述**:
```go
rows, err := auth.db.Query(
    "SELECT ... WHERE ... AND (NULLIF($5,'') IS NULL OR created_at >= to_timestamp(NULLIF($6,'')::bigint))",
    user, user, taskID, taskID, fromTS, fromTS, toTS, toTS, limit
)
```

雖然使用了參數化查詢，但 `NULLIF($6,'')::bigint` 的類型轉換可能在某些情況下導致 SQL 錯誤或意外行為。

**修復建議**:
```go
// 在 Go 層面驗證和轉換參數
var fromTime *time.Time
var toTime *time.Time

if fromTS != "" {
    if ts, err := strconv.ParseInt(fromTS, 10, 64); err == nil {
        t := time.Unix(ts, 0)
        fromTime = &t
    }
}

if toTS != "" {
    if ts, err := strconv.ParseInt(toTS, 10, 64); err == nil {
        t := time.Unix(ts, 0)
        toTime = &t
    }
}

// 使用更安全的查詢
query := "SELECT ... WHERE (payer=$1 OR payee=$2)"
args := []interface{}{user, user}

if taskID != "" {
    query += " AND task_id=$" + strconv.Itoa(len(args)+1)
    args = append(args, taskID)
}

if fromTime != nil {
    query += " AND created_at >= $" + strconv.Itoa(len(args)+1)
    args = append(args, fromTime)
}
```

**優先級**: P1 - 高優先級

---

### 🟡 MED-02: 密碼哈希升級邏輯存在競態條件

**位置**: Line 268-272  
**嚴重程度**: 中危  
**CVSS 評分**: 5.3

**問題描述**:
```go
if !strings.HasPrefix(pw, "$2") {
    if h, hErr := hashPassword(password); hErr == nil {
        _, _ = u.db.Exec("UPDATE users SET password=$1 WHERE username=$2", h, username)
    }
}
```

多個並發登錄請求可能導致：
- 競態條件（多次哈希更新）
- 數據庫寫入競爭
- 潛在的密碼不一致

**修復建議**:
```go
// 使用事務和行鎖
tx, err := u.db.Begin()
if err != nil {
    return &pb.LoginResponse{Success: false, StatusMessage: "db error"}, nil
}
defer tx.Rollback()

var pw string
err = tx.QueryRow("SELECT password FROM users WHERE username = $1 FOR UPDATE", username).Scan(&pw)
if err != nil || !verifyPassword(pw, password) {
    return &pb.LoginResponse{Success: false, StatusMessage: "invalid credentials"}, nil
}

if !strings.HasPrefix(pw, "$2") {
    if h, hErr := hashPassword(password); hErr == nil {
        _, _ = tx.Exec("UPDATE users SET password=$1 WHERE username=$2", h, username)
    }
}

if err := tx.Commit(); err != nil {
    return &pb.LoginResponse{Success: false, StatusMessage: "db error"}, nil
}
```

**優先級**: P2 - 中優先級

---

### 🟡 MED-03: 日誌文件權限過於寬鬆

**位置**: Line 161  
**嚴重程度**: 中危  
**CVSS 評分**: 4.3

**問題描述**:
```go
f, err := os.OpenFile(fname, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
```

日誌文件使用 `0644` 權限（所有用戶可讀），可能洩露敏感信息：
- 用戶名
- IP 地址
- 任務 ID
- 系統內部狀態

**修復建議**:
```go
f, err := os.OpenFile(fname, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
```

**優先級**: P2 - 中優先級

---

### 🟡 MED-04: CORS 配置過於寬鬆

**位置**: 多處（Line 1579, 1602, 1626 等）  
**嚴重程度**: 中危  
**CVSS 評分**: 5.0

**問題描述**:
```go
w.Header().Set("Access-Control-Allow-Origin", "*")
```

允許所有來源訪問 API，可能導致：
- CSRF 攻擊
- 未授權的跨域請求
- 數據洩露

**修復建議**:
```go
// 使用環境變量配置允許的來源
allowedOrigins := strings.Split(os.Getenv("NODEPOOL_ALLOWED_ORIGINS"), ",")
if len(allowedOrigins) == 0 {
    allowedOrigins = []string{"http://localhost:3000", "http://localhost:3001"}
}

origin := r.Header.Get("Origin")
for _, allowed := range allowedOrigins {
    if origin == strings.TrimSpace(allowed) {
        w.Header().Set("Access-Control-Allow-Origin", origin)
        break
    }
}
```

**優先級**: P2 - 中優先級

---

### 🟢 LOW-01: 缺少請求速率限制

**位置**: 所有 HTTP 端點  
**嚴重程度**: 低危  
**CVSS 評分**: 3.7

**問題描述**:
所有 API 端點都沒有速率限制，可能導致：
- 暴力破解攻擊（登錄端點）
- API 濫用
- 資源耗盡

**修復建議**:
```go
import "golang.org/x/time/rate"

// 創建速率限制器
type rateLimiter struct {
    limiters map[string]*rate.Limiter
    mu       sync.RWMutex
}

func (rl *rateLimiter) getLimiter(ip string) *rate.Limiter {
    rl.mu.Lock()
    defer rl.mu.Unlock()
    
    limiter, exists := rl.limiters[ip]
    if !exists {
        limiter = rate.NewLimiter(10, 20) // 10 req/s, burst 20
        rl.limiters[ip] = limiter
    }
    return limiter
}

func rateLimitMiddleware(rl *rateLimiter, next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        ip := strings.Split(r.RemoteAddr, ":")[0]
        limiter := rl.getLimiter(ip)
        
        if !limiter.Allow() {
            w.WriteHeader(http.StatusTooManyRequests)
            json.NewEncoder(w).Encode(map[string]any{
                "success": false,
                "status_message": "rate limit exceeded",
            })
            return
        }
        
        next.ServeHTTP(w, r)
    })
}
```

**優先級**: P3 - 低優先級

---

### 🟢 LOW-02: 缺少請求大小限制

**位置**: 所有 HTTP 端點  
**嚴重程度**: 低危  
**CVSS 評分**: 3.1

**問題描述**:
除了文件上傳端點，其他端點沒有請求體大小限制，可能導致：
- 大型 JSON 負載攻擊
- 內存耗盡

**修復建議**:
```go
func maxBytesMiddleware(maxBytes int64) func(http.Handler) http.Handler {
    return func(next http.Handler) http.Handler {
        return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            r.Body = http.MaxBytesReader(w, r.Body, maxBytes)
            next.ServeHTTP(w, r)
        })
    }
}

// 應用到所有端點
mux.Handle("/api/", maxBytesMiddleware(1*1024*1024)(handler)) // 1MB 限制
```

**優先級**: P3 - 低優先級

---

## 性能問題

### PERF-01: 內存占用過高（文件上傳）

**問題**: `io.ReadAll` 將整個文件加載到內存  
**影響**: 100MB 文件 = 100MB 內存占用  
**建議**: 使用流式處理或臨時文件

### PERF-02: 數據庫查詢未使用索引

**問題**: Line 1708 的複雜查詢可能缺少適當索引  
**建議**: 
```sql
CREATE INDEX idx_transfers_payer_created ON cpt_transfers(payer, created_at DESC);
CREATE INDEX idx_transfers_payee_created ON cpt_transfers(payee, created_at DESC);
CREATE INDEX idx_transfers_task_id ON cpt_transfers(task_id);
```

### PERF-03: Redis 連接未使用連接池

**問題**: 每次操作都可能創建新連接  
**建議**: 已使用 `redis.NewClient`，確保配置連接池參數

---

## 安全最佳實踐建議

### 1. 認證與授權
- ✅ 使用 JWT 進行身份驗證
- ✅ 使用 bcrypt 進行密碼哈希
- ❌ 缺少 Token 刷新機制的速率限制
- ❌ 缺少多因素認證（MFA）

### 2. 輸入驗證
- ✅ 使用參數化查詢防止 SQL 注入
- ✅ 驗證文件類型（僅 .zip）
- ⚠️ 部分端點缺少輸入長度驗證
- ❌ 缺少請求體大小限制

### 3. 數據保護
- ✅ 使用 PostgreSQL 存儲敏感數據
- ✅ 密碼使用 bcrypt 哈希
- ❌ JWT Secret 有硬編碼默認值
- ❌ 日誌文件權限過於寬鬆

### 4. 網絡安全
- ⚠️ CORS 配置過於寬鬆（允許所有來源）
- ❌ 缺少 HTTPS 強制（應在反向代理層實施）
- ❌ 缺少請求速率限制

### 5. 錯誤處理
- ✅ 錯誤消息不洩露敏感信息
- ✅ 使用結構化日誌記錄
- ⚠️ 部分錯誤處理使用 `_` 忽略錯誤

---

## 修復優先級總結

### P0 - 立即修復（1-3 天）
1. HIGH-01: 文件上傳大小限制
2. HIGH-02: JWT Secret 硬編碼

### P1 - 高優先級（1 週內）
3. MED-01: SQL 注入風險

### P2 - 中優先級（2 週內）
4. MED-02: 密碼哈希競態條件
5. MED-03: 日誌文件權限
6. MED-04: CORS 配置

### P3 - 低優先級（1 個月內）
7. LOW-01: 請求速率限制
8. LOW-02: 請求大小限制

---

## 測試建議

### 安全測試
```bash
# 1. 測試文件上傳大小限制
curl -X POST http://localhost:8081/api/create-torrent \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@large_file.zip"

# 2. 測試 JWT 驗證
curl -X GET http://localhost:8081/api/balance \
  -H "Authorization: Bearer invalid_token"

# 3. 測試 SQL 注入
curl -X GET "http://localhost:8081/api/transfers?from_ts='; DROP TABLE users; --"

# 4. 測試 CORS
curl -X OPTIONS http://localhost:8081/api/login \
  -H "Origin: http://evil.com" \
  -H "Access-Control-Request-Method: POST"
```

### 性能測試
```bash
# 使用 Apache Bench 測試並發性能
ab -n 1000 -c 10 -H "Authorization: Bearer $TOKEN" \
  http://localhost:8081/api/tasks
```

---

## 合規性檢查

### OWASP Top 10 (2021)
- ✅ A01: Broken Access Control - 已實施 JWT 認證
- ⚠️ A02: Cryptographic Failures - JWT Secret 有默認值
- ✅ A03: Injection - 使用參數化查詢
- ⚠️ A04: Insecure Design - CORS 配置過於寬鬆
- ⚠️ A05: Security Misconfiguration - 日誌權限、默認密鑰
- ✅ A06: Vulnerable Components - 使用最新依賴
- ✅ A07: Authentication Failures - 使用 bcrypt
- ❌ A08: Software and Data Integrity - 缺少完整性檢查
- ✅ A09: Security Logging - 已實施日誌記錄
- ❌ A10: SSRF - 未檢查（NodePool 不發起外部請求）

---

## 結論

NodePool 服務的整體安全性**中等**。主要問題集中在：

1. **認證安全**: JWT Secret 硬編碼是最嚴重的問題
2. **資源保護**: 文件上傳缺少適當的大小驗證
3. **配置安全**: CORS、日誌權限等配置需要加強

**建議立即修復 P0 級別的問題**，然後按優先級逐步改進其他問題。

**預計修復時間**: 
- P0 問題: 1-2 天
- P1-P2 問題: 1-2 週
- P3 問題: 2-4 週

**風險評估**: 
- 當前風險等級: **中高**
- 修復後風險等級: **低**

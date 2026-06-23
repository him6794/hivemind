# Master API 模組

**Master API** 是 HiveMind 的核心對外接口服務，提供 RESTful API 和 WebSocket 通信功能。

## 主要功能

- 提供 RESTful API 接口
- 處理用戶認證和授權
- 管理任務提交和查詢
- 協調 Node Manager 和 Task Scheduler
- 提供 WebSocket 實時通信

## API 端點

### 任務相關

- `POST /api/tasks` - 提交新任務
- `GET /api/tasks/{id}` - 查詢任務狀態
- `GET /api/tasks` - 列表所有任務

### 節點管理

- `GET /api/nodes` - 列表所有工作節點
- `GET /api/nodes/{id}` - 查詢節點狀態

### 認證

- `POST /api/auth/login` - 登錄
- `POST /api/auth/refresh` - 刷新令牌
- `GET /api/auth/me` - 獲取當前用戶信息

## 配置

主配置文件路徑: `config/master-api.toml`

```toml
[server]
host = "0.0.0.0"
port = 8080

[database]
url = "postgres://hivemind:password@localhost/hivemind"

[auth]
jwt_secret = "your-secret-key"
jwt_expires_in = "86400"
```

## 運行

```bash
# 以開發模式運行
cargo run --bin hivemind master-api

# 構建發布版本
cargo build --release --bin hivemind
```

## 開發

### 代碼結構

```text
master-api/
├── src/
│   ├── controllers/    # 控制器邏輯
│   ├── models/         # 數據模型
│   ├── services/       # 業務邏輯
│   ├── middlewares/    # 中間件
│   ├── routes.rs       # 路由定義
│   └── lib.rs          # 庫入口
├── Cargo.toml          # 包清單
└── README.md           # 本文檔
```

### 添加新 API

1. 創建新控制器文件 `src/controllers/<module>.rs`
2. 定義路由 `src/routes.rs`
3. 編寫業務邏輯 `src/services/<module>.rs`

---

# Master API Module

**Master API** is the core external interface service of HiveMind, providing RESTful API and WebSocket communication.

## Key Features

- RESTful API interface
- User authentication and authorization
- Task submission and query management
- Coordination between Node Manager and Task Scheduler
- WebSocket real-time communication

## API Endpoints

### Task Related

- `POST /api/tasks` - Submit new task
- `GET /api/tasks/{id}` - Query task status
- `GET /api/tasks` - List all tasks

### Node Management

- `GET /api/nodes` - List all worker nodes
- `GET /api/nodes/{id}` - Query node status

### Authentication

- `POST /api/auth/login` - Login
- `POST /api/auth/refresh` - Refresh token
- `GET /api/auth/me` - Get current user info

## Configuration

Main config file path: `config/master-api.toml`

```toml
[server]
host = "0.0.0.0"
port = 8080

[database]
url = "postgres://hivemind:password@localhost/hivemind"

[auth]
jwt_secret = "your-secret-key"
jwt_expires_in = "86400"
```

## Running

```bash
# Run in development mode
cargo run --bin hivemind master-api

# Build release version
cargo build --release --bin hivemind
```

## Development

### Code Structure

```text
master-api/
├── src/
│   ├── controllers/    # Controller logic
│   ├── models/         # Data models
│   ├── services/       # Business logic
│   ├── middlewares/    # Middlewares
│   ├── routes.rs       # Route definitions
│   └── lib.rs          # Library entry
├── Cargo.toml          # Package manifest
└── README.md           # This document
```

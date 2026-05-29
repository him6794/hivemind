# Hivemind - Distributed Compute Runtime

A distributed compute runtime for public-network workers, now rewritten in Rust.

## Quick Start

```bash
# Build
make build

# Run tests
make test

# Start development environment
make dev
```

## Architecture

Hivemind is a batch-oriented distributed compute runtime. The system consists of:

- **Hivemind Binary** (`hivemind-rs/`) - Unified Rust binary containing all services
- **Frontend** (`frontend/`) - React UIs for master and worker management
- **Infrastructure** - Docker Compose for Redis and PostgreSQL

### Services

All services run in a single binary (`hivemind-bin`):

| Service | Port | Protocol | Description |
|---------|------|----------|-------------|
| Master API | 8082 | HTTP | User authentication, task management |
| Nodepool | 50051 | gRPC | Worker registration, task scheduling |
| Worker | 50053 | gRPC | Task execution, result reporting |

### Rust Crates

```
hivemind-rs/
├── crates/
│   ├── auth/           - JWT authentication
│   ├── common/         - Shared utilities
│   ├── config/         - Configuration management
│   ├── database/       - PostgreSQL & Redis integration
│   ├── hivemind-bin/   - Main binary entry point
│   ├── master-api/     - HTTP API handlers
│   ├── models/         - Data models
│   ├── node-manager/   - Worker management
│   ├── proto/          - gRPC protobuf definitions
│   ├── task-scheduler/ - Task dispatch & scheduling
│   ├── vpn-service/    - VPN management
│   └── worker-executor/- Task execution engine
```

## Development

### Prerequisites

- Rust 1.70+
- Docker (for Redis & PostgreSQL)
- Node.js 18+ (for frontend)

### Build

```bash
# Build Rust binary
make build

# Build frontend
make build-frontend
```

### Test

```bash
# Run all tests
make test

# Run tests with verbose output
make test-verbose
```

### Lint & Format

```bash
# Run linter
make lint

# Format code
make fmt
```

## Deployment

### Docker Compose

```bash
# Start all services
make docker-up

# View logs
make docker-logs

# Stop all services
make docker-down
```

### Manual

```bash
# Start Redis
docker run -d -p 6379:6379 redis:7-alpine

# Start PostgreSQL
docker run -d -p 5432:5432 \
  -e POSTGRES_DB=hivemind \
  -e POSTGRES_USER=hivemind \
  -e POSTGRES_PASSWORD=hivemind \
  postgres:16-alpine

# Run Hivemind
DATABASE_URL=postgres://hivemind:hivemind@localhost:5432/hivemind \
REDIS_URL=redis://localhost:6379 \
./target/release/hivemind-bin all
```

## Configuration

Configuration is via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | - | PostgreSQL connection string |
| `REDIS_URL` | - | Redis connection string |
| `JWT_SECRET` | - | JWT signing secret |
| `LOG_LEVEL` | `info` | Log level (debug, info, warn, error) |

## API Reference

### Authentication

```bash
# Login
curl -X POST http://localhost:8082/api/login \
  -H "Content-Type: application/json" \
  -d '{"username": "testuser", "password": "testpass123"}'
```

### Tasks

```bash
# Create task
curl -X POST http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"task_id": "task-1", "payload": "..."}'

# List tasks
curl http://localhost:8082/api/tasks \
  -H "Authorization: Bearer <token>"
```

### Health Check

```bash
curl http://localhost:8082/health
```

## License

MIT

# NodePool Progress

## Goal
Build the NodePool service with worker registration, task administration, worker management, and a thin Master proxy layer.

## Status
In progress.

## Current Step
The NodePool gRPC and HTTP surface is in place, but the worker trust, persistence, and observability layers are still simplified. The next focus is to move from the in-memory prototype toward a production-ready backend.

## Completed
- Implemented an in-memory `WorkerRepository`.
  - File: [services/nodepool/internal/repository/worker_repository.go](services/nodepool/internal/repository/worker_repository.go)
  - Supports `SaveWorker`, `GetWorker`, `ListWorkers`, `DeleteWorker`, and `UpdateHeartbeat`.
- Implemented `WorkerService` as the business logic layer.
  - File: [services/nodepool/internal/service/worker_service.go](services/nodepool/internal/service/worker_service.go)
  - Handles `RegisterWorker`, `Heartbeat`, `ListAvailableWorkers`, and `RemoveWorker`.
  - Heartbeat timeout is 30 seconds; workers are marked offline after timeout.
- Implemented transport handlers that connect HTTP/gRPC requests to the service layer.
  - File: [services/nodepool/internal/handler/worker_handler.go](services/nodepool/internal/handler/worker_handler.go)
  - Exposes `Register`, `Heartbeat`, `List`, and `Remove`.
- Added the minimal gRPC wrapper and server wiring.
  - File: [services/nodepool/pb/hivemind.pb.go](services/nodepool/pb/hivemind.pb.go)
  - File: [services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
  - `NodeManagerService` currently exposes `RegisterWorkerNode` and `ReportStatus`.
- Added HTTP endpoints for NodePool and Master integration.
  - NodePool HTTP:
    - `GET /api/tasks` with `status`, `q`, `from_ts`, `to_ts`, `limit`, `offset`, `sort_by`, and `order`.
    - `POST /api/stop-tasks`
    - `GET /api/workers`
    - `POST /api/remove-worker`
  - Master HTTP proxies to NodePool for `GET /api/tasks` and also exposes `POST /api/stop-tasks` and `POST /api/remove-worker`.
  - Files:
    - [services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)
    - [services/master/cmd/server/main.go](services/master/cmd/server/main.go)
- Wired the frontend UI entry points.
  - Master UI handles worker status and task management.
  - Worker UI handles worker registration and worker control.
  - Files: [frontend/master-ui/src/App.jsx](frontend/master-ui/src/App.jsx) and [frontend/worker-ui/src/App.jsx](frontend/worker-ui/src/App.jsx)
- Added repository and service tests.
  - Repository tests: [services/nodepool/internal/repository/worker_repository_test.go](services/nodepool/internal/repository/worker_repository_test.go)
  - Service tests: [services/nodepool/internal/service/worker_service_test.go](services/nodepool/internal/service/worker_service_test.go)
  - `go test ./...` under `services/nodepool` passes.

## Open Work
- The current `pb/hivemind.pb.go` is still a handwritten wrapper and should be replaced by real `protoc` generated Go code.
- `Handler -> Service -> repository.Worker` is still wired against the current wrapper and will need a proper mapping layer when generated protobuf types are introduced.
- The worker repository is still in-memory, so durability and restart recovery are not yet solved.
- RBAC and ACL enforcement are still missing.
- Metrics and tracing are still missing.

## Next
1. Introduce real protobuf generation with `protoc`, `protoc-gen-go`, and `protoc-gen-go-grpc`.
2. Switch the gRPC surface to generated types and validate it with smoke tests such as `grpcurl`.
3. Replace the in-memory repository with a durable backend such as Redis or Postgres.
4. Add authentication middleware, then add logging middleware with `slog` or `zap`.
5. Add CI workflows for `go test`, `go vet`, and `go build`.

## Useful Commands
```bash
cd services/nodepool
go test ./...
```

```bash
cd services/nodepool
go run ./cmd/server
```

The current gRPC entry points are:
- `nodepool.NodeManagerService.RegisterWorkerNode`
- `nodepool.NodeManagerService.ReportStatus`

## Risks
- The current `pb/hivemind.pb.go` wrapper is a stopgap and should not be mistaken for final protobuf generation.
- The handler and service layers still depend on the current wrapper and need a future mapping layer when generated protobuf types are introduced.
- In-memory storage is acceptable for early development, but it will not survive process restarts or scale-out scenarios.

## Reference Files
- [proto/hivemind.proto](proto/hivemind.proto)
- [services/nodepool/go.mod](services/nodepool/go.mod)
- [services/nodepool/internal/repository/worker_repository.go](services/nodepool/internal/repository/worker_repository.go)
- [services/nodepool/internal/service/worker_service.go](services/nodepool/internal/service/worker_service.go)
- [services/nodepool/internal/handler/worker_handler.go](services/nodepool/internal/handler/worker_handler.go)
- [services/nodepool/pb/hivemind.pb.go](services/nodepool/pb/hivemind.pb.go)
- [services/nodepool/cmd/server/main.go](services/nodepool/cmd/server/main.go)

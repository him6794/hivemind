package handler

import (
    "context"
    "time"

    "hivemind/services/nodepool/internal/repository"
    "hivemind/services/nodepool/internal/service"
)

// The types below mirror what protobuf-generated types would provide.
// They keep the handler decoupled from transport so later substitution is simple.

type RegisterWorkerRequest struct {
    Id   string
    Addr string
    Meta map[string]string
}

type RegisterWorkerResponse struct {
    Ok    bool
    Error string
}

type HeartbeatRequest struct {
    Id string
}

type HeartbeatResponse struct {
    Ok    bool
    Error string
}

type WorkerInfo struct {
    Id            string
    Addr          string
    Meta          map[string]string
    Status        string
    LastHeartbeat int64 // unix seconds
}

type ListWorkersRequest struct{}

type ListWorkersResponse struct {
    Workers []WorkerInfo
}

// WorkerHandler adapts transport-level requests to service calls.
type WorkerHandler struct {
    svc *service.WorkerService
}

// NewWorkerHandler creates a new handler backed by the provided service.
func NewWorkerHandler(svc *service.WorkerService) *WorkerHandler {
    return &WorkerHandler{svc: svc}
}

// Register handles worker registration (transport layer -> service layer).
func (h *WorkerHandler) Register(ctx context.Context, req *RegisterWorkerRequest) (*RegisterWorkerResponse, error) {
    w := &repository.Worker{
        ID:   req.Id,
        Addr: req.Addr,
        Meta: req.Meta,
    }
    if err := h.svc.RegisterWorker(ctx, w); err != nil {
        return &RegisterWorkerResponse{Ok: false, Error: err.Error()}, nil
    }
    return &RegisterWorkerResponse{Ok: true}, nil
}

// Heartbeat updates heartbeat for a worker.
func (h *WorkerHandler) Heartbeat(ctx context.Context, req *HeartbeatRequest) (*HeartbeatResponse, error) {
    if err := h.svc.Heartbeat(ctx, req.Id); err != nil {
        return &HeartbeatResponse{Ok: false, Error: err.Error()}, nil
    }
    return &HeartbeatResponse{Ok: true}, nil
}

// ListAvailable returns workers considered available by the service.
func (h *WorkerHandler) ListAvailable(ctx context.Context, req *ListWorkersRequest) (*ListWorkersResponse, error) {
    ws, err := h.svc.ListAvailableWorkers(ctx)
    if err != nil {
        return nil, err
    }
    out := make([]WorkerInfo, 0, len(ws))
    for _, w := range ws {
        if w == nil {
            continue
        }
        out = append(out, WorkerInfo{
            Id:            w.ID,
            Addr:          w.Addr,
            Meta:          w.Meta,
            Status:        w.Status,
            LastHeartbeat: w.LastHeartbeat.Unix(),
        })
    }
    return &ListWorkersResponse{Workers: out}, nil
}

// Remove deletes a worker by id.
func (h *WorkerHandler) Remove(ctx context.Context, id string) error {
    return h.svc.RemoveWorker(ctx, id)
}

// Helper: this function can be used by real gRPC adapter to map proto timestamp.
func unixToTime(sec int64) time.Time {
    return time.Unix(sec, 0)
}

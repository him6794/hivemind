package repository

import (
    "errors"
    "sync"
    "time"
)

// Worker represents a worker node stored in the in-memory repository.
// Ensure this struct stays compatible with your project's internal models.
type Worker struct {
    ID            string            `json:"id"`
    Addr          string            `json:"addr"`
    Meta          map[string]string `json:"meta,omitempty"`
    Status        string            `json:"status,omitempty"`
    LastHeartbeat time.Time         `json:"last_heartbeat"`
}

// WorkerRepository provides thread-safe in-memory storage for Worker objects.
type WorkerRepository struct {
    mu      sync.RWMutex
    workers map[string]*Worker
}

// NewWorkerRepository creates a new WorkerRepository instance.
func NewWorkerRepository() *WorkerRepository {
    return &WorkerRepository{
        workers: make(map[string]*Worker),
    }
}

var (
    // ErrWorkerNotFound is returned when a worker cannot be located.
    ErrWorkerNotFound = errors.New("worker not found")
)

// SaveWorker saves or updates a worker in the repository.
// It makes an internal copy of the provided Worker to avoid external mutation.
func (r *WorkerRepository) SaveWorker(w *Worker) error {
    if w == nil {
        return errors.New("worker is nil")
    }
    if w.ID == "" {
        return errors.New("worker id is empty")
    }

    r.mu.Lock()
    defer r.mu.Unlock()

    copyW := copyWorker(w)
    r.workers[w.ID] = copyW
    return nil
}

// GetWorker retrieves a worker by id. The returned Worker is a copy.
// The bool return value indicates whether the worker was found.
func (r *WorkerRepository) GetWorker(id string) (*Worker, bool) {
    r.mu.RLock()
    defer r.mu.RUnlock()

    w, ok := r.workers[id]
    if !ok {
        return nil, false
    }
    return copyWorker(w), true
}

// ListWorkers returns a slice with copies of all workers currently stored.
func (r *WorkerRepository) ListWorkers() []*Worker {
    r.mu.RLock()
    defer r.mu.RUnlock()

    out := make([]*Worker, 0, len(r.workers))
    for _, w := range r.workers {
        out = append(out, copyWorker(w))
    }
    return out
}

// DeleteWorker removes a worker by id. Returns ErrWorkerNotFound if missing.
func (r *WorkerRepository) DeleteWorker(id string) error {
    r.mu.Lock()
    defer r.mu.Unlock()

    if _, ok := r.workers[id]; !ok {
        return ErrWorkerNotFound
    }
    delete(r.workers, id)
    return nil
}

// UpdateHeartbeat updates the LastHeartbeat timestamp for a worker.
// Returns ErrWorkerNotFound if the worker does not exist.
func (r *WorkerRepository) UpdateHeartbeat(id string, t time.Time) error {
    r.mu.Lock()
    defer r.mu.Unlock()

    w, ok := r.workers[id]
    if !ok {
        return ErrWorkerNotFound
    }
    // update in-place
    w.LastHeartbeat = t
    // optionally set status to alive
    if w.Status == "" {
        w.Status = "alive"
    }
    return nil
}

// copyWorker returns a deep-ish copy of a Worker. Meta map is copied.
func copyWorker(in *Worker) *Worker {
    if in == nil {
        return nil
    }
    var meta map[string]string
    if in.Meta != nil {
        meta = make(map[string]string, len(in.Meta))
        for k, v := range in.Meta {
            meta[k] = v
        }
    }
    out := &Worker{
        ID:            in.ID,
        Addr:          in.Addr,
        Meta:          meta,
        Status:        in.Status,
        LastHeartbeat: in.LastHeartbeat,
    }
    return out
}

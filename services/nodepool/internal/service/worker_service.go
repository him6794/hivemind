package service

import (
	"context"
	"errors"
	"time"

	"hivemind/services/nodepool/internal/repository"
)

// WorkerService handles business logic for workers.
type WorkerService struct {
	repo             *repository.WorkerRepository
	heartbeatTimeout time.Duration
}

// NewWorkerService creates a new WorkerService with the given repository.
func NewWorkerService(repo *repository.WorkerRepository) *WorkerService {
	return &WorkerService{
		repo:             repo,
		heartbeatTimeout: 30 * time.Second,
	}
}

var (
	// ErrInvalidWorker indicates invalid worker registration data.
	ErrInvalidWorker = errors.New("invalid worker")
)

// RegisterWorker registers or updates a worker. Sets status = ACTIVE and updates heartbeat.
func (s *WorkerService) RegisterWorker(ctx context.Context, w *repository.Worker) error {
	if ctx == nil {
		ctx = context.Background()
	}
	if w == nil || w.ID == "" || w.Addr == "" {
		return ErrInvalidWorker
	}

	// set status and heartbeat
	w.Status = "ACTIVE"
	w.LastHeartbeat = time.Now()

	return s.repo.SaveWorker(w)
}

// Heartbeat updates the LastHeartbeat for the given worker id.
func (s *WorkerService) Heartbeat(ctx context.Context, id string) error {
	if ctx == nil {
		ctx = context.Background()
	}
	if id == "" {
		return ErrInvalidWorker
	}

	return s.repo.UpdateHeartbeat(id, time.Now())
}

// ListAvailableWorkers returns workers considered active (heartbeat within timeout).
// Workers with stale heartbeats will be marked OFFLINE in the repository.
func (s *WorkerService) ListAvailableWorkers(ctx context.Context) ([]*repository.Worker, error) {
	if ctx == nil {
		ctx = context.Background()
	}

	all := s.repo.ListWorkers()
	now := time.Now()
	var avail []*repository.Worker

	for _, w := range all {
		if w == nil || w.ID == "" {
			continue
		}
		age := now.Sub(w.LastHeartbeat)
		if age <= s.heartbeatTimeout && w.Status == "ACTIVE" {
			avail = append(avail, w)
			continue
		}
		// mark offline if stale
		if age > s.heartbeatTimeout && w.Status != "OFFLINE" {
			w.Status = "OFFLINE"
			// persist status change
			_ = s.repo.SaveWorker(w)
		}
	}
	return avail, nil
}

// ListWorkers returns all workers and refreshes status by heartbeat age.
// If includeOffline is false, only ACTIVE workers are returned.
func (s *WorkerService) ListWorkers(ctx context.Context, includeOffline bool) ([]*repository.Worker, error) {
	if ctx == nil {
		ctx = context.Background()
	}

	all := s.repo.ListWorkers()
	now := time.Now()
	out := make([]*repository.Worker, 0, len(all))

	for _, w := range all {
		if w == nil || w.ID == "" {
			continue
		}
		age := now.Sub(w.LastHeartbeat)
		if age > s.heartbeatTimeout {
			if w.Status != "OFFLINE" {
				w.Status = "OFFLINE"
				_ = s.repo.SaveWorker(w)
			}
		} else if w.Status == "" {
			w.Status = "ACTIVE"
			_ = s.repo.SaveWorker(w)
		}

		if includeOffline || w.Status == "ACTIVE" {
			out = append(out, w)
		}
	}

	return out, nil
}

// GetWorker returns a copy of a worker by id without mutating worker state.
func (s *WorkerService) GetWorker(ctx context.Context, id string) (*repository.Worker, bool) {
	if ctx == nil {
		ctx = context.Background()
	}
	if id == "" {
		return nil, false
	}
	return s.repo.GetWorker(id)
}

// RemoveWorker deletes a worker from repository.
func (s *WorkerService) RemoveWorker(ctx context.Context, id string) error {
	if ctx == nil {
		ctx = context.Background()
	}
	if id == "" {
		return ErrInvalidWorker
	}
	return s.repo.DeleteWorker(id)
}

// MarkWorkerOffline marks an existing worker as OFFLINE.
// This is used when active health probes fail even if heartbeat is still fresh.
func (s *WorkerService) MarkWorkerOffline(ctx context.Context, id string) error {
	if ctx == nil {
		ctx = context.Background()
	}
	if id == "" {
		return ErrInvalidWorker
	}
	w, ok := s.repo.GetWorker(id)
	if !ok || w == nil {
		return repository.ErrWorkerNotFound
	}
	if w.Status == "OFFLINE" {
		return nil
	}
	w.Status = "OFFLINE"
	return s.repo.SaveWorker(w)
}

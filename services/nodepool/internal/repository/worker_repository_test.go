package repository

import (
    "testing"
    "time"
)

func TestWorkerRepository_CRUD_Heartbeat(t *testing.T) {
    repo := NewWorkerRepository()

    w := &Worker{
        ID:   "worker-1",
        Addr: "127.0.0.1:9000",
        Meta: map[string]string{"os": "linux"},
    }

    if err := repo.SaveWorker(w); err != nil {
        t.Fatalf("SaveWorker failed: %v", err)
    }

    got, ok := repo.GetWorker("worker-1")
    if !ok || got == nil {
        t.Fatalf("GetWorker missing after save")
    }

    if got.ID != w.ID || got.Addr != w.Addr {
        t.Fatalf("GetWorker returned wrong data: %+v", got)
    }

    list := repo.ListWorkers()
    if len(list) != 1 {
        t.Fatalf("ListWorkers expected 1 got %d", len(list))
    }

    // Update heartbeat
    now := time.Now()
    if err := repo.UpdateHeartbeat("worker-1", now); err != nil {
        t.Fatalf("UpdateHeartbeat failed: %v", err)
    }
    g2, ok := repo.GetWorker("worker-1")
    if !ok {
        t.Fatalf("GetWorker missing after heartbeat")
    }
    if g2.LastHeartbeat.Before(now) {
        t.Fatalf("LastHeartbeat not updated: %v vs %v", g2.LastHeartbeat, now)
    }

    // Delete
    if err := repo.DeleteWorker("worker-1"); err != nil {
        t.Fatalf("DeleteWorker failed: %v", err)
    }
    if _, ok := repo.GetWorker("worker-1"); ok {
        t.Fatalf("worker still present after delete")
    }
}

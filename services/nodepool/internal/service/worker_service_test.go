package service

import (
    "context"
    "testing"
    "time"

    "hivemind/services/nodepool/internal/repository"
)

func TestWorkerService_Register_Heartbeat_List_Remove(t *testing.T) {
    repo := repository.NewWorkerRepository()
    svc := NewWorkerService(repo)

    ctx := context.Background()

    w := &repository.Worker{ID: "w1", Addr: "127.0.0.1:9000"}
    if err := svc.RegisterWorker(ctx, w); err != nil {
        t.Fatalf("RegisterWorker failed: %v", err)
    }

    got, ok := repo.GetWorker("w1")
    if !ok {
        t.Fatalf("worker missing after register")
    }
    if got.Status != "ACTIVE" {
        t.Fatalf("expected ACTIVE status, got %s", got.Status)
    }

    // simulate heartbeat
    time.Sleep(5 * time.Millisecond)
    if err := svc.Heartbeat(ctx, "w1"); err != nil {
        t.Fatalf("Heartbeat failed: %v", err)
    }

    before := time.Now()
    ws, err := svc.ListAvailableWorkers(ctx)
    if err != nil {
        t.Fatalf("ListAvailableWorkers error: %v", err)
    }
    if len(ws) == 0 {
        t.Fatalf("expected available workers to include w1")
    }

    // make worker stale and ensure it becomes OFFLINE
    stale := repository.Worker{ID: "w2", Addr: "127.0.0.2:9000", LastHeartbeat: time.Now().Add(-1 * time.Minute), Status: "ACTIVE"}
    if err := repo.SaveWorker(&stale); err != nil {
        t.Fatalf("SaveWorker stale failed: %v", err)
    }

    ws2, err := svc.ListAvailableWorkers(ctx)
    if err != nil {
        t.Fatalf("ListAvailableWorkers error: %v", err)
    }
    // w2 should not be in available list
    for _, x := range ws2 {
        if x.ID == "w2" {
            t.Fatalf("stale worker w2 returned as available")
        }
    }

    // verify status marked OFFLINE in repo
    s2, ok := repo.GetWorker("w2")
    if !ok {
        t.Fatalf("w2 should exist in repo")
    }
    if s2.Status != "OFFLINE" {
        t.Fatalf("expected w2 OFFLINE got %s", s2.Status)
    }

    // remove w1
    if err := svc.RemoveWorker(ctx, "w1"); err != nil {
        t.Fatalf("RemoveWorker failed: %v", err)
    }
    if _, ok := repo.GetWorker("w1"); ok {
        t.Fatalf("w1 still exists after remove")
    }

    _ = before // silence unused when running short
}

package service

import (
	"context"
	"testing"
)

func TestTaskService_ExecuteAndStateTransitions(t *testing.T) {
	t.Parallel()

	svc := NewTaskService()
	ctx := context.Background()

	if err := svc.ExecuteTask(ctx, "task-1", "magnet:?xt=urn:btih:demo", 0.1, 0.2, 8, 4); err != nil {
		t.Fatalf("ExecuteTask failed: %v", err)
	}

	svc.mu.RLock()
	task, ok := svc.tasks["task-1"]
	svc.mu.RUnlock()
	if !ok {
		t.Fatalf("expected task-1 to exist")
	}
	if task.Status != TaskStatusRunning {
		t.Fatalf("expected RUNNING, got %s", task.Status)
	}

	if err := svc.UploadOutput(ctx, "task-1", "line-1"); err != nil {
		t.Fatalf("UploadOutput failed: %v", err)
	}
	out, err := svc.GetOutput(ctx, "task-1")
	if err != nil {
		t.Fatalf("GetOutput failed: %v", err)
	}
	if out != "line-1" {
		t.Fatalf("unexpected output: %q", out)
	}

	if err := svc.UpdateUsage(ctx, "task-1", 0.3, 0.4, 0.5, 0.6); err != nil {
		t.Fatalf("UpdateUsage failed: %v", err)
	}

	if err := svc.UploadResult(ctx, "task-1", "magnet:?xt=urn:btih:result"); err != nil {
		t.Fatalf("UploadResult failed: %v", err)
	}

	svc.mu.RLock()
	task = svc.tasks["task-1"]
	svc.mu.RUnlock()
	if task.Status != TaskStatusCompleted {
		t.Fatalf("expected COMPLETED, got %s", task.Status)
	}
	if task.ResultTorrent == "" {
		t.Fatalf("result torrent should not be empty")
	}

	if err := svc.StopTask(ctx, "task-1"); err != nil {
		t.Fatalf("StopTask failed: %v", err)
	}
	svc.mu.RLock()
	stopped := svc.tasks["task-1"].Status
	svc.mu.RUnlock()
	if stopped != TaskStatusStopped {
		t.Fatalf("expected STOPPED, got %s", stopped)
	}
}

func TestTaskService_ValidationAndNotFound(t *testing.T) {
	t.Parallel()

	svc := NewTaskService()
	ctx := context.Background()

	if err := svc.ExecuteTask(ctx, "", "magnet:?xt=urn:btih:demo", 0, 0, 1, 1); err == nil {
		t.Fatalf("expected error for empty task id")
	}
	if err := svc.ExecuteTask(ctx, "task-2", "", 0, 0, 1, 1); err == nil {
		t.Fatalf("expected error for empty torrent")
	}

	if err := svc.UploadOutput(ctx, "missing", "x"); err == nil {
		t.Fatalf("expected not found on UploadOutput")
	}
	if _, err := svc.GetOutput(ctx, "missing"); err == nil {
		t.Fatalf("expected not found on GetOutput")
	}
	if err := svc.UploadResult(ctx, "missing", "magnet"); err == nil {
		t.Fatalf("expected not found on UploadResult")
	}
	if err := svc.StopTask(ctx, "missing"); err == nil {
		t.Fatalf("expected not found on StopTask")
	}
	if err := svc.UpdateUsage(ctx, "missing", 0, 0, 0, 0); err == nil {
		t.Fatalf("expected not found on UpdateUsage")
	}
}

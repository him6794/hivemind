package main

import (
	"context"
	"strings"
	"testing"
	"time"

	"hivemind/services/nodepool/internal/repository"
	"hivemind/services/nodepool/internal/service"
	"hivemind/services/nodepool/pb"
)

func newTestBatchMaster() *masterNodeServer {
	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	return &masterNodeServer{
		svc:          svc,
		taskToWorker: make(map[string]string),
		taskRoutes:   make(map[string]map[string]string),
		tasks:        make(map[string]*taskState),
		batchLeases:  make(map[string]*batchLease),
	}
}

func TestPullBatchAppliesBackpressureAndDoesNotPersistFullCacheState(t *testing.T) {
	m := newTestBatchMaster()
	ctx := context.Background()
	if err := m.svc.RegisterWorker(ctx, &repository.Worker{ID: "worker-1", Addr: "127.0.0.1:50053"}); err != nil {
		t.Fatalf("register worker: %v", err)
	}

	m.tasks["task-small"] = &taskState{
		TaskID:        "task-small",
		Owner:         "user-1",
		Status:        "PENDING",
		TorrentSource: "artifact://task-code-small",
		ReqMemoryGB:   2,
		Priority:      10,
		Deterministic: true,
		SideEffects:   false,
	}

	fullCacheDigest := strings.Repeat("artifact-digest-", 100)
	resp, err := m.PullBatch(ctx, &pb.PullBatchRequest{
		WorkerId:           "worker-1",
		MaxInflightBatches: 1,
		AvailableMemoryGb:  1,
		QueueCapacity:      1,
		CacheSummary: &pb.CacheSummary{
			CacheEpoch:         7,
			BloomFilter:        []byte{1, 2, 3, 4},
			PartialCacheDigest: fullCacheDigest,
			ArtifactCount:      500000,
			TotalBytes:         1 << 40,
		},
	})
	if err != nil {
		t.Fatalf("PullBatch low memory err: %v", err)
	}
	if !resp.GetSuccess() {
		t.Fatalf("PullBatch failed: %s", resp.GetStatusMessage())
	}
	if len(resp.GetTasks()) != 0 {
		t.Fatalf("expected no tasks when available memory is insufficient, got %d", len(resp.GetTasks()))
	}

	resp, err = m.PullBatch(ctx, &pb.PullBatchRequest{
		WorkerId:           "worker-1",
		MaxInflightBatches: 1,
		AvailableMemoryGb:  4,
		QueueCapacity:      0,
	})
	if err != nil {
		t.Fatalf("PullBatch full queue err: %v", err)
	}
	if len(resp.GetTasks()) != 0 {
		t.Fatalf("expected no tasks when queue capacity is zero, got %d", len(resp.GetTasks()))
	}

	resp, err = m.PullBatch(ctx, &pb.PullBatchRequest{
		WorkerId:           "worker-1",
		MaxInflightBatches: 1,
		AvailableMemoryGb:  4,
		QueueCapacity:      2,
		CacheSummary: &pb.CacheSummary{
			CacheEpoch:         8,
			BloomFilter:        []byte{9, 9, 9},
			PartialCacheDigest: fullCacheDigest,
			ArtifactCount:      500000,
			TotalBytes:         1 << 40,
		},
	})
	if err != nil {
		t.Fatalf("PullBatch err: %v", err)
	}
	if !resp.GetSuccess() {
		t.Fatalf("PullBatch failed: %s", resp.GetStatusMessage())
	}
	if len(resp.GetTasks()) != 1 {
		t.Fatalf("expected one leased task, got %d", len(resp.GetTasks()))
	}
	if got := resp.GetTasks()[0].GetTaskId(); got != "task-small" {
		t.Fatalf("leased task=%q, want task-small", got)
	}
	if got := resp.GetTasks()[0].GetExecutionPackage().GetTaskCodeRef(); got != "artifact://task-code-small" {
		t.Fatalf("task code ref=%q, want artifact://task-code-small", got)
	}
	if got := m.tasks["task-small"].Status; got != "DISPATCHED" {
		t.Fatalf("leased task status=%q, want DISPATCHED so timeout recovery can manage it", got)
	}

	worker, ok := m.svc.GetWorker(ctx, "worker-1")
	if !ok {
		t.Fatal("worker missing after PullBatch")
	}
	if worker.Meta != nil {
		for k, v := range worker.Meta {
			if strings.Contains(k, "cache") || strings.Contains(v, fullCacheDigest) {
				t.Fatalf("nodepool persisted cache state in worker metadata: %s=%s", k, v)
			}
		}
	}
}

func TestPullBatchRespectsInflightBatchLimit(t *testing.T) {
	m := newTestBatchMaster()
	ctx := context.Background()
	if err := m.svc.RegisterWorker(ctx, &repository.Worker{ID: "worker-1", Addr: "127.0.0.1:50053"}); err != nil {
		t.Fatalf("register worker: %v", err)
	}
	m.tasks["task-1"] = &taskState{TaskID: "task-1", Status: "PENDING", TorrentSource: "artifact://task-1", ReqMemoryGB: 1}
	m.tasks["task-2"] = &taskState{TaskID: "task-2", Status: "PENDING", TorrentSource: "artifact://task-2", ReqMemoryGB: 1}

	first, err := m.PullBatch(ctx, &pb.PullBatchRequest{WorkerId: "worker-1", MaxInflightBatches: 1, AvailableMemoryGb: 8, QueueCapacity: 1})
	if err != nil {
		t.Fatalf("first PullBatch err: %v", err)
	}
	if len(first.GetTasks()) != 1 {
		t.Fatalf("expected first pull to lease one task, got %d", len(first.GetTasks()))
	}

	second, err := m.PullBatch(ctx, &pb.PullBatchRequest{WorkerId: "worker-1", MaxInflightBatches: 1, AvailableMemoryGb: 8, QueueCapacity: 1})
	if err != nil {
		t.Fatalf("second PullBatch err: %v", err)
	}
	if len(second.GetTasks()) != 0 {
		t.Fatalf("expected inflight limit to suppress second lease, got %d tasks", len(second.GetTasks()))
	}
}

func TestCompleteBatchRecordsExecutionMetricsAndAllowsPartialFailure(t *testing.T) {
	m := newTestBatchMaster()
	ctx := context.Background()
	if err := m.svc.RegisterWorker(ctx, &repository.Worker{ID: "worker-1", Addr: "127.0.0.1:50053"}); err != nil {
		t.Fatalf("register worker: %v", err)
	}
	m.tasks["task-ok"] = &taskState{TaskID: "task-ok", Status: "PENDING", TorrentSource: "artifact://ok", ReqMemoryGB: 1}
	m.tasks["task-fail"] = &taskState{TaskID: "task-fail", Status: "PENDING", TorrentSource: "artifact://fail", ReqMemoryGB: 1}

	pull, err := m.PullBatch(ctx, &pb.PullBatchRequest{WorkerId: "worker-1", MaxInflightBatches: 2, AvailableMemoryGb: 8, QueueCapacity: 2})
	if err != nil {
		t.Fatalf("PullBatch err: %v", err)
	}
	if len(pull.GetTasks()) != 2 {
		t.Fatalf("expected two leased tasks, got %d", len(pull.GetTasks()))
	}

	complete, err := m.CompleteBatch(ctx, &pb.CompleteBatchRequest{
		WorkerId: "worker-1",
		BatchId:  pull.GetBatchId(),
		Tasks: []*pb.CompletedTask{
			{
				TaskId:             "task-ok",
				Status:             "COMPLETED",
				StdoutArtifactRef:  "artifact://stdout-ok",
				ResultArtifactRefs: []string{"artifact://result-ok"},
				Metrics: &pb.ExecutionMetrics{
					CpuTimeMs:     1200,
					WallTimeMs:    1500,
					PeakMemoryMb:  256,
					DownloadBytes: 4096,
					CacheHits:     2,
				},
			},
			{
				TaskId:            "task-fail",
				Status:            "FAILED",
				StderrArtifactRef: "artifact://stderr-fail",
				Metrics: &pb.ExecutionMetrics{
					CpuTimeMs:    500,
					WallTimeMs:   700,
					PeakMemoryMb: 128,
				},
			},
		},
	})
	if err != nil {
		t.Fatalf("CompleteBatch err: %v", err)
	}
	if !complete.GetSuccess() {
		t.Fatalf("CompleteBatch failed: %s", complete.GetStatusMessage())
	}
	if m.tasks["task-ok"].Status != "COMPLETED" {
		t.Fatalf("task-ok status=%s, want COMPLETED", m.tasks["task-ok"].Status)
	}
	if m.tasks["task-fail"].Status != "FAILED" {
		t.Fatalf("task-fail status=%s, want FAILED", m.tasks["task-fail"].Status)
	}
	if got := m.tasks["task-ok"].WallTimeMS; got != 1500 {
		t.Fatalf("task-ok wall time=%d, want 1500", got)
	}
	if got := m.tasks["task-ok"].CacheHits; got != 2 {
		t.Fatalf("task-ok cache hits=%d, want 2", got)
	}
	if _, ok := m.batchLeases[pull.GetBatchId()]; ok {
		t.Fatal("batch lease was not cleared after completion")
	}
}

func TestDAGPolicyAllowsSpeculativeExecutionOnlyForDeterministicSideEffectFreeTasks(t *testing.T) {
	deterministic := &pb.DAGNode{
		TaskId:        "task-cacheable",
		Deterministic: true,
		SideEffects:   false,
		MaxRetries:    3,
		DeadlineUnix:  time.Now().Add(time.Hour).Unix(),
		Priority:      100,
	}
	if !allowsSpeculativeExecution(deterministic) {
		t.Fatal("deterministic side-effect-free task should allow speculative execution")
	}

	sideEffecting := &pb.DAGNode{
		TaskId:        "task-side-effect",
		Deterministic: true,
		SideEffects:   true,
		MaxRetries:    3,
	}
	if allowsSpeculativeExecution(sideEffecting) {
		t.Fatal("side-effecting task must not allow speculative execution")
	}

	nondeterministic := &pb.DAGNode{
		TaskId:        "task-random",
		Deterministic: false,
		SideEffects:   false,
		MaxRetries:    3,
	}
	if allowsSpeculativeExecution(nondeterministic) {
		t.Fatal("non-deterministic task must not allow speculative execution")
	}
}

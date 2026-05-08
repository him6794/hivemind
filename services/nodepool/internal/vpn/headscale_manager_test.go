package vpn

import (
	"context"
	"testing"
	"time"
)

func TestNewHeadscaleManager(t *testing.T) {
	tests := []struct {
		name    string
		config  *Config
		wantErr bool
	}{
		{
			name:    "nil config",
			config:  nil,
			wantErr: true,
		},
		{
			name: "missing ServerURL",
			config: &Config{
				IPPrefix:   "100.64.0.0/10",
				BaseDomain: "hivemind.local",
			},
			wantErr: true,
		},
		{
			name: "missing IPPrefix",
			config: &Config{
				ServerURL:  "http://localhost:8080",
				BaseDomain: "hivemind.local",
			},
			wantErr: true,
		},
		{
			name: "missing BaseDomain",
			config: &Config{
				ServerURL: "http://localhost:8080",
				IPPrefix:  "100.64.0.0/10",
			},
			wantErr: true,
		},
		{
			name: "invalid IP prefix",
			config: &Config{
				ServerURL:  "http://localhost:8080",
				IPPrefix:   "invalid",
				BaseDomain: "hivemind.local",
			},
			wantErr: true,
		},
		{
			name: "valid config",
			config: &Config{
				ServerURL:      "http://localhost:8080",
				IPPrefix:       "100.64.0.0/10",
				BaseDomain:     "hivemind.local",
				EphemeralNodes: true,
				NodeExpiry:     24 * time.Hour,
				DBType:         "sqlite",
				DBPath:         "/tmp/test.db",
			},
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			manager, err := NewHeadscaleManager(tt.config)
			if (err != nil) != tt.wantErr {
				t.Errorf("NewHeadscaleManager() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr && manager == nil {
				t.Error("NewHeadscaleManager() returned nil manager")
			}
		})
	}
}

func TestRegisterWorker(t *testing.T) {
	config := &Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	ctx := context.Background()

	t.Run("register new worker", func(t *testing.T) {
		worker, err := manager.RegisterWorker(ctx, "worker-1", "worker1.local")
		if err != nil {
			t.Fatalf("RegisterWorker() error = %v", err)
		}

		if worker.WorkerID != "worker-1" {
			t.Errorf("WorkerID = %v, want worker-1", worker.WorkerID)
		}

		if worker.Hostname != "worker1.local" {
			t.Errorf("Hostname = %v, want worker1.local", worker.Hostname)
		}

		if worker.VirtualIP == "" {
			t.Error("VirtualIP is empty")
		}

		if worker.AuthKey == "" {
			t.Error("AuthKey is empty")
		}

		if !worker.Online {
			t.Error("Worker should be online")
		}
	})

	t.Run("register existing worker", func(t *testing.T) {
		// 第二次註冊同一個 Worker
		worker, err := manager.RegisterWorker(ctx, "worker-1", "worker1.local")
		if err != nil {
			t.Fatalf("RegisterWorker() error = %v", err)
		}

		if worker.WorkerID != "worker-1" {
			t.Errorf("WorkerID = %v, want worker-1", worker.WorkerID)
		}
	})

	t.Run("register multiple workers", func(t *testing.T) {
		worker2, err := manager.RegisterWorker(ctx, "worker-2", "worker2.local")
		if err != nil {
			t.Fatalf("RegisterWorker() error = %v", err)
		}

		worker3, err := manager.RegisterWorker(ctx, "worker-3", "worker3.local")
		if err != nil {
			t.Fatalf("RegisterWorker() error = %v", err)
		}

		// 確保分配了不同的 IP
		if worker2.VirtualIP == worker3.VirtualIP {
			t.Error("Workers should have different virtual IPs")
		}
	})
}

func TestUnregisterWorker(t *testing.T) {
	config := &Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	ctx := context.Background()

	// 註冊一個 Worker
	_, err = manager.RegisterWorker(ctx, "worker-1", "worker1.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	t.Run("unregister existing worker", func(t *testing.T) {
		err := manager.UnregisterWorker(ctx, "worker-1")
		if err != nil {
			t.Errorf("UnregisterWorker() error = %v", err)
		}
	})

	t.Run("unregister non-existing worker", func(t *testing.T) {
		err := manager.UnregisterWorker(ctx, "worker-999")
		if err == nil {
			t.Error("UnregisterWorker() should return error for non-existing worker")
		}
	})
}

func TestUpdateWorkerStatus(t *testing.T) {
	config := &Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	ctx := context.Background()

	// 註冊一個 Worker
	_, err = manager.RegisterWorker(ctx, "worker-1", "worker1.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	t.Run("update status to offline", func(t *testing.T) {
		err := manager.UpdateWorkerStatus(ctx, "worker-1", false)
		if err != nil {
			t.Errorf("UpdateWorkerStatus() error = %v", err)
		}
	})

	t.Run("update status to online", func(t *testing.T) {
		err := manager.UpdateWorkerStatus(ctx, "worker-1", true)
		if err != nil {
			t.Errorf("UpdateWorkerStatus() error = %v", err)
		}
	})

	t.Run("update non-existing worker", func(t *testing.T) {
		err := manager.UpdateWorkerStatus(ctx, "worker-999", true)
		if err == nil {
			t.Error("UpdateWorkerStatus() should return error for non-existing worker")
		}
	})
}

func TestAssignWorkerToTask(t *testing.T) {
	config := &Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	ctx := context.Background()

	// 註冊兩個 Worker
	_, err = manager.RegisterWorker(ctx, "worker-1", "worker1.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	_, err = manager.RegisterWorker(ctx, "worker-2", "worker2.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	t.Run("assign workers to task", func(t *testing.T) {
		err := manager.AssignWorkerToTask(ctx, "task-1", "worker-1")
		if err != nil {
			t.Errorf("AssignWorkerToTask() error = %v", err)
		}

		err = manager.AssignWorkerToTask(ctx, "task-1", "worker-2")
		if err != nil {
			t.Errorf("AssignWorkerToTask() error = %v", err)
		}
	})

	t.Run("assign non-existing worker", func(t *testing.T) {
		err := manager.AssignWorkerToTask(ctx, "task-1", "worker-999")
		if err == nil {
			t.Error("AssignWorkerToTask() should return error for non-existing worker")
		}
	})
}

func TestGetTaskPeers(t *testing.T) {
	config := &Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	ctx := context.Background()

	// 註冊三個 Worker
	_, err = manager.RegisterWorker(ctx, "worker-1", "worker1.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	_, err = manager.RegisterWorker(ctx, "worker-2", "worker2.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	_, err = manager.RegisterWorker(ctx, "worker-3", "worker3.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	// 分配到任務
	manager.AssignWorkerToTask(ctx, "task-1", "worker-1")
	manager.AssignWorkerToTask(ctx, "task-1", "worker-2")
	manager.AssignWorkerToTask(ctx, "task-1", "worker-3")

	t.Run("get task peers", func(t *testing.T) {
		peers, err := manager.GetTaskPeers(ctx, "task-1", "worker-1")
		if err != nil {
			t.Fatalf("GetTaskPeers() error = %v", err)
		}

		// worker-1 請求，應該返回 worker-2 和 worker-3
		if len(peers) != 2 {
			t.Errorf("GetTaskPeers() returned %d peers, want 2", len(peers))
		}

		// 確保不包含請求者自己
		for _, peer := range peers {
			if peer.WorkerID == "worker-1" {
				t.Error("GetTaskPeers() should not include requester")
			}
		}
	})

	t.Run("get peers for non-existing task", func(t *testing.T) {
		peers, err := manager.GetTaskPeers(ctx, "task-999", "worker-1")
		if err != nil {
			t.Fatalf("GetTaskPeers() error = %v", err)
		}

		if len(peers) != 0 {
			t.Errorf("GetTaskPeers() returned %d peers, want 0", len(peers))
		}
	})
}

func TestRemoveWorkerFromTask(t *testing.T) {
	config := &Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	ctx := context.Background()

	// 註冊 Worker 並分配到任務
	_, err = manager.RegisterWorker(ctx, "worker-1", "worker1.local")
	if err != nil {
		t.Fatalf("RegisterWorker() error = %v", err)
	}

	manager.AssignWorkerToTask(ctx, "task-1", "worker-1")

	t.Run("remove worker from task", func(t *testing.T) {
		err := manager.RemoveWorkerFromTask(ctx, "task-1", "worker-1")
		if err != nil {
			t.Errorf("RemoveWorkerFromTask() error = %v", err)
		}

		// 驗證已移除
		peers, _ := manager.GetTaskPeers(ctx, "task-1", "worker-2")
		for _, peer := range peers {
			if peer.WorkerID == "worker-1" {
				t.Error("Worker should be removed from task")
			}
		}
	})
}

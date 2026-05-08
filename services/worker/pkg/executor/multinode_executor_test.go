package executor

import (
	"context"
	"testing"
	"time"

	"hivemind/services/worker/pkg/vpn"
)

// mockExecutor is a mock implementation of Executor for testing
type mockExecutor struct {
	executeFunc func(ctx context.Context, task *Task) (*TaskResult, error)
}

func (m *mockExecutor) Execute(ctx context.Context, task *Task) (*TaskResult, error) {
	if m.executeFunc != nil {
		return m.executeFunc(ctx, task)
	}
	return &TaskResult{
		Success: true,
		Stdout:  "mock result",
	}, nil
}

// mockVPNManager is a mock implementation of VPN manager for testing
type mockVPNManager struct {
	peers []*vpn.PeerInfo
}

func (m *mockVPNManager) GetTaskPeers(taskID string) ([]*vpn.PeerInfo, error) {
	return m.peers, nil
}

func (m *mockVPNManager) GetTsnetClient() *vpn.TsnetClient {
	return &vpn.TsnetClient{}
}

func TestNewMultinodeExecutor(t *testing.T) {
	mockVPN := &mockVPNManager{}
	mockExec := &mockExecutor{}

	executor := NewMultinodeExecutor(mockVPN, mockExec)

	if executor == nil {
		t.Fatal("Expected non-nil executor")
	}

	if executor.vpnManager == nil {
		t.Error("VPN manager should not be nil")
	}

	if executor.localExecutor == nil {
		t.Error("Local executor should not be nil")
	}
}

func TestExecuteLocalTask(t *testing.T) {
	mockVPN := &mockVPNManager{}
	mockExec := &mockExecutor{
		executeFunc: func(ctx context.Context, task *Task) (*TaskResult, error) {
			return &TaskResult{
				TaskID:  task.ID,
				Success: true,
				Stdout:  "local execution",
			}, nil
		},
	}

	executor := NewMultinodeExecutor(mockVPN, mockExec)

	task := &Task{
		ID:            "task-1",
		Type:          "compute",
		Payload:       []byte("test payload"),
		RequiresPeers: false,
	}

	ctx := context.Background()
	result, err := executor.Execute(ctx, task)

	if err != nil {
		t.Fatalf("Execute failed: %v", err)
	}

	if !result.Success {
		t.Error("Expected successful result")
	}

	if result.Stdout != "local execution" {
		t.Errorf("Expected output 'local execution', got '%s'", result.Stdout)
	}
}

func TestExecuteMultinodeTask(t *testing.T) {
	mockVPN := &mockVPNManager{
		peers: []*vpn.PeerInfo{
			{
				WorkerID:  "worker-2",
				Hostname:  "worker-2",
				VirtualIP: "100.64.0.2",
			},
			{
				WorkerID:  "worker-3",
				Hostname:  "worker-3",
				VirtualIP: "100.64.0.3",
			},
		},
	}

	mockExec := &mockExecutor{
		executeFunc: func(ctx context.Context, task *Task) (*TaskResult, error) {
			return &TaskResult{
				TaskID:  task.ID,
				Success: true,
				Stdout:  "multinode execution",
			}, nil
		},
	}

	executor := NewMultinodeExecutor(mockVPN, mockExec)

	task := &Task{
		ID:            "task-1",
		Type:          "distributed-compute",
		Payload:       []byte("test payload"),
		RequiresPeers: true,
		PeerWorkers:   []string{"worker-2", "worker-3"},
	}

	ctx := context.Background()
	result, err := executor.Execute(ctx, task)

	if err != nil {
		t.Fatalf("Execute failed: %v", err)
	}

	if !result.Success {
		t.Error("Expected successful result")
	}
}

func TestExecuteMultinodeTaskNoPeers(t *testing.T) {
	mockVPN := &mockVPNManager{
		peers: []*vpn.PeerInfo{}, // No peers available
	}

	mockExec := &mockExecutor{
		executeFunc: func(ctx context.Context, task *Task) (*TaskResult, error) {
			return &TaskResult{
				TaskID:  task.ID,
				Success: true,
				Stdout:  "fallback to local",
			}, nil
		},
	}

	executor := NewMultinodeExecutor(mockVPN, mockExec)

	task := &Task{
		ID:            "task-1",
		Type:          "distributed-compute",
		Payload:       []byte("test payload"),
		RequiresPeers: true,
	}

	ctx := context.Background()
	result, err := executor.Execute(ctx, task)

	if err != nil {
		t.Fatalf("Execute failed: %v", err)
	}

	if !result.Success {
		t.Error("Expected successful result")
	}

	// Should fall back to local execution
	if result.Stdout != "fallback to local" {
		t.Errorf("Expected fallback to local execution")
	}
}

func TestGetPeerList(t *testing.T) {
	mockVPN := &mockVPNManager{
		peers: []*vpn.PeerInfo{
			{
				WorkerID:  "worker-2",
				Hostname:  "worker-2",
				VirtualIP: "100.64.0.2",
			},
		},
	}

	mockExec := &mockExecutor{}
	executor := NewMultinodeExecutor(mockVPN, mockExec)

	peers, err := executor.GetPeerList("task-1")
	if err != nil {
		t.Fatalf("GetPeerList failed: %v", err)
	}

	if len(peers) != 1 {
		t.Errorf("Expected 1 peer, got %d", len(peers))
	}

	if peers[0].WorkerID != "worker-2" {
		t.Errorf("Expected worker-2, got %s", peers[0].WorkerID)
	}
}

func TestExecuteWithContext(t *testing.T) {
	mockVPN := &mockVPNManager{}
	mockExec := &mockExecutor{
		executeFunc: func(ctx context.Context, task *Task) (*TaskResult, error) {
			// Simulate long-running task
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-time.After(100 * time.Millisecond):
				return &TaskResult{Success: true}, nil
			}
		},
	}

	executor := NewMultinodeExecutor(mockVPN, mockExec)

	task := &Task{
		ID:            "task-1",
		Type:          "compute",
		RequiresPeers: false,
	}

	// Test with cancelled context
	ctx, cancel := context.WithCancel(context.Background())
	cancel() // Cancel immediately

	_, err := executor.Execute(ctx, task)
	if err == nil {
		t.Error("Expected error with cancelled context")
	}
}

func TestExecuteWithTimeout(t *testing.T) {
	mockVPN := &mockVPNManager{}
	mockExec := &mockExecutor{
		executeFunc: func(ctx context.Context, task *Task) (*TaskResult, error) {
			// Simulate long-running task
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-time.After(2 * time.Second):
				return &TaskResult{Success: true}, nil
			}
		},
	}

	executor := NewMultinodeExecutor(mockVPN, mockExec)

	task := &Task{
		ID:            "task-1",
		Type:          "compute",
		RequiresPeers: false,
	}

	// Test with timeout
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	_, err := executor.Execute(ctx, task)
	if err == nil {
		t.Error("Expected timeout error")
	}
}

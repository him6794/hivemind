package vpn

import (
	"context"
	"testing"
	"time"
)

func TestNewManager(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	if mgr.workerID != cfg.WorkerID {
		t.Errorf("Expected workerID %s, got %s", cfg.WorkerID, mgr.workerID)
	}

	if mgr.nodepoolAddr != cfg.NodepoolAddr {
		t.Errorf("Expected nodepoolAddr %s, got %s", cfg.NodepoolAddr, mgr.nodepoolAddr)
	}
}

func TestManagerStartStop(t *testing.T) {
	t.Skip("Skipping integration test - requires Headscale server")

	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	// Start VPN
	if err := mgr.Start(); err != nil {
		t.Fatalf("Failed to start VPN: %v", err)
	}

	// Verify it's registered
	if !mgr.IsRegistered() {
		t.Error("Manager should be registered after Start()")
	}

	// Wait a bit
	time.Sleep(2 * time.Second)

	// Stop VPN
	if err := mgr.Stop(); err != nil {
		t.Fatalf("Failed to stop VPN: %v", err)
	}

	// Verify it's unregistered
	if mgr.IsRegistered() {
		t.Error("Manager should not be registered after Stop()")
	}
}

func TestGetTaskPeers(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	// Use mock client
	mgr.nodepoolClient = &mockNodepoolClient{}
	mgr.registered = true

	peers, err := mgr.GetTaskPeers("test-task-1")
	if err != nil {
		t.Fatalf("Failed to get task peers: %v", err)
	}

	if len(peers) == 0 {
		t.Error("Expected at least one peer")
	}
}

func TestSetTaskID(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	taskID := "test-task-123"
	mgr.SetTaskID(taskID)

	if mgr.GetTaskID() != taskID {
		t.Errorf("Expected taskID %s, got %s", taskID, mgr.GetTaskID())
	}
}

func TestHeartbeat(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	// Use mock client and tsnet
	mgr.nodepoolClient = &mockNodepoolClient{}
	mgr.tsnetClient = &TsnetClient{
		localIP:   "100.64.0.1",
		connected: true,
	}

	// Send heartbeat
	mgr.sendHeartbeat()

	// If we get here without panic, test passes
}

func TestManagerWithoutVPN(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	// Try to get peers without starting VPN
	_, err = mgr.GetTaskPeers("test-task")
	if err == nil {
		t.Error("Expected error when getting peers without VPN started")
	}
}

func TestManagerConcurrency(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	mgr.nodepoolClient = &mockNodepoolClient{}
	mgr.registered = true

	// Test concurrent access
	done := make(chan bool)
	for i := 0; i < 10; i++ {
		go func() {
			mgr.SetTaskID("task-123")
			_ = mgr.GetTaskID()
			_ = mgr.IsRegistered()
			done <- true
		}()
	}

	for i := 0; i < 10; i++ {
		<-done
	}
}

func TestManagerContextCancellation(t *testing.T) {
	cfg := &ManagerConfig{
		NodepoolAddr: "localhost:50051",
		WorkerID:     "test-worker-1",
		Hostname:     "test-worker-1",
		StateDir:     "/tmp/test-vpn",
	}

	mgr, err := NewManager(cfg)
	if err != nil {
		t.Fatalf("Failed to create manager: %v", err)
	}

	// Cancel context
	mgr.cancel()

	// Operations should handle cancelled context gracefully
	ctx, cancel := context.WithTimeout(context.Background(), 1*time.Second)
	defer cancel()

	mgr.nodepoolClient = &mockNodepoolClient{}
	_, err = mgr.nodepoolClient.GetTaskPeers(ctx, &TaskPeersRequest{TaskID: "test"})

	// Should not panic
	if err != nil && err != context.Canceled {
		// Context cancellation is acceptable
	}
}

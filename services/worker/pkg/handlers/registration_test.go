package handlers

import (
	"context"
	"testing"
)

func TestNewWorkerRegistration(t *testing.T) {
	vpnMgr := &mockVPNManager{}
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, vpnMgr)

	if reg == nil {
		t.Fatal("Expected non-nil registration")
	}

	if reg.workerID != workerID {
		t.Errorf("Expected workerID %s, got %s", workerID, reg.workerID)
	}

	if reg.vpnManager == nil {
		t.Error("VPN manager should not be nil")
	}
}

func TestRegister(t *testing.T) {
	vpnMgr := &mockVPNManager{}
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, vpnMgr)

	ctx := context.Background()
	err := reg.Register(ctx)

	if err != nil {
		t.Fatalf("Register failed: %v", err)
	}

	if !reg.IsRegistered() {
		t.Error("Worker should be registered after Register()")
	}

	if !vpnMgr.started {
		t.Error("VPN should be started after Register()")
	}
}

func TestRegisterTwice(t *testing.T) {
	vpnMgr := &mockVPNManager{}
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, vpnMgr)

	ctx := context.Background()

	// First registration
	err := reg.Register(ctx)
	if err != nil {
		t.Fatalf("First Register failed: %v", err)
	}

	// Second registration should fail
	err = reg.Register(ctx)
	if err == nil {
		t.Error("Expected error when registering twice")
	}
}

func TestUnregister(t *testing.T) {
	vpnMgr := &mockVPNManager{}
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, vpnMgr)

	ctx := context.Background()

	// Register first
	err := reg.Register(ctx)
	if err != nil {
		t.Fatalf("Register failed: %v", err)
	}

	// Then unregister
	err = reg.Unregister(ctx)
	if err != nil {
		t.Fatalf("Unregister failed: %v", err)
	}

	if reg.IsRegistered() {
		t.Error("Worker should not be registered after Unregister()")
	}

	if vpnMgr.started {
		t.Error("VPN should be stopped after Unregister()")
	}
}

func TestUnregisterWithoutRegister(t *testing.T) {
	vpnMgr := &mockVPNManager{}
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, vpnMgr)

	ctx := context.Background()

	// Unregister without registering first
	err := reg.Unregister(ctx)
	if err != nil {
		t.Errorf("Unregister should not fail when not registered: %v", err)
	}
}

func TestRegisterWithoutVPN(t *testing.T) {
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, nil)

	ctx := context.Background()
	err := reg.Register(ctx)

	// Should succeed even without VPN
	if err != nil {
		t.Fatalf("Register failed: %v", err)
	}

	if !reg.IsRegistered() {
		t.Error("Worker should be registered")
	}
}

func TestIsRegistered(t *testing.T) {
	vpnMgr := &mockVPNManager{}
	workerID := "test-worker-1"

	reg := NewWorkerRegistration(workerID, vpnMgr)

	// Initially not registered
	if reg.IsRegistered() {
		t.Error("Worker should not be registered initially")
	}

	// After registration
	ctx := context.Background()
	reg.Register(ctx)

	if !reg.IsRegistered() {
		t.Error("Worker should be registered after Register()")
	}

	// After unregistration
	reg.Unregister(ctx)

	if reg.IsRegistered() {
		t.Error("Worker should not be registered after Unregister()")
	}
}

// mockVPNManager for testing
type mockVPNManager struct {
	started bool
}

func (m *mockVPNManager) Start() error {
	m.started = true
	return nil
}

func (m *mockVPNManager) Stop() error {
	m.started = false
	return nil
}


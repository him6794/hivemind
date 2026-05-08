package handlers

import (
	"context"
	"fmt"
	"log"
)

// VPNManager interface for VPN operations
type VPNManager interface {
	Start() error
	Stop() error
}

// WorkerRegistration handles worker registration lifecycle
type WorkerRegistration struct {
	vpnManager VPNManager
	workerID   string
	registered bool
}

// NewWorkerRegistration creates a new registration handler
func NewWorkerRegistration(workerID string, vpnMgr VPNManager) *WorkerRegistration {
	return &WorkerRegistration{
		vpnManager: vpnMgr,
		workerID:   workerID,
	}
}

// Register registers the worker with Nodepool and joins VPN
func (r *WorkerRegistration) Register(ctx context.Context) error {
	if r.registered {
		return fmt.Errorf("worker already registered")
	}

	log.Printf("[Registration] Registering worker %s", r.workerID)

	// TODO: Call nodepool RegisterWorkerNode RPC via agent client
	// This would include worker capabilities, resources, etc.

	// Start VPN connection
	if r.vpnManager != nil {
		log.Printf("[Registration] Starting VPN connection")
		if err := r.vpnManager.Start(); err != nil {
			return fmt.Errorf("failed to start VPN: %w", err)
		}
		log.Printf("[Registration] VPN connected successfully")
	}

	r.registered = true
	log.Printf("[Registration] Worker %s registered successfully", r.workerID)

	return nil
}

// Unregister unregisters the worker and leaves VPN
func (r *WorkerRegistration) Unregister(ctx context.Context) error {
	if !r.registered {
		return nil
	}

	log.Printf("[Registration] Unregistering worker %s", r.workerID)

	// Stop VPN connection
	if r.vpnManager != nil {
		log.Printf("[Registration] Stopping VPN connection")
		if err := r.vpnManager.Stop(); err != nil {
			log.Printf("[Registration] Failed to stop VPN: %v", err)
		}
	}

	// TODO: Call nodepool UnregisterWorkerNode RPC

	r.registered = false
	log.Printf("[Registration] Worker %s unregistered successfully", r.workerID)

	return nil
}

// IsRegistered returns whether the worker is registered
func (r *WorkerRegistration) IsRegistered() bool {
	return r.registered
}

// RegisterWorker is the legacy function for compatibility
// TODO: implement Register/Unregister logic using generated proto messages
func RegisterWorker(req interface{}) (interface{}, error) {
	// call nodepool RegisterWorkerNode RPC via agent client
	return nil, nil
}

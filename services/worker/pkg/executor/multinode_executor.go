package executor

import (
	"context"
	"fmt"
	"log"
	"net"
	"time"

	"hivemind/services/worker/pkg/vpn"
)

// VPNManager interface for VPN operations
type VPNManager interface {
	GetTaskPeers(taskID string) ([]*vpn.PeerInfo, error)
	GetTsnetClient() *vpn.TsnetClient
}

// MultinodeExecutor handles task execution across multiple workers via VPN
type MultinodeExecutor struct {
	vpnManager    VPNManager
	localExecutor Executor
}

// Executor interface for local task execution
type Executor interface {
	Execute(ctx context.Context, task *Task) (*TaskResult, error)
}

// Task represents a task to be executed
type Task struct {
	ID            string
	Type          string
	Payload       []byte
	RequiresPeers bool
	PeerWorkers   []string
}

// NewMultinodeExecutor creates a new multinode executor
func NewMultinodeExecutor(vpnMgr VPNManager, localExec Executor) *MultinodeExecutor {
	return &MultinodeExecutor{
		vpnManager:    vpnMgr,
		localExecutor: localExec,
	}
}

// Execute runs a task, coordinating with peer workers if needed
func (e *MultinodeExecutor) Execute(ctx context.Context, task *Task) (*TaskResult, error) {
	log.Printf("[Multinode Executor] Executing task %s (type: %s)", task.ID, task.Type)

	// If task doesn't require peers, execute locally
	if !task.RequiresPeers {
		return e.localExecutor.Execute(ctx, task)
	}

	// Get peer information from VPN manager
	peers, err := e.vpnManager.GetTaskPeers(task.ID)
	if err != nil {
		return nil, fmt.Errorf("failed to get task peers: %w", err)
	}

	if len(peers) == 0 {
		log.Printf("[Multinode Executor] No peers found, executing locally")
		return e.localExecutor.Execute(ctx, task)
	}

	log.Printf("[Multinode Executor] Found %d peers for task %s", len(peers), task.ID)

	// Verify connectivity to peers
	if err := e.verifyPeerConnectivity(ctx, peers); err != nil {
		log.Printf("[Multinode Executor] Peer connectivity check failed: %v", err)
		// Continue anyway, some peers might still be reachable
	}

	// Execute task with peer coordination
	return e.executeWithPeers(ctx, task, peers)
}

// verifyPeerConnectivity checks if we can reach peer workers
func (e *MultinodeExecutor) verifyPeerConnectivity(ctx context.Context, peers []*vpn.PeerInfo) error {
	tsnetClient := e.vpnManager.GetTsnetClient()
	if tsnetClient == nil {
		return fmt.Errorf("VPN client not available")
	}

	for _, peer := range peers {
		// Try to dial peer's worker service
		address := fmt.Sprintf("%s:50052", peer.VirtualIP)

		dialCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
		conn, err := tsnetClient.DialContext(dialCtx, "tcp", address)
		cancel()

		if err != nil {
			log.Printf("[Multinode Executor] Failed to connect to peer %s (%s): %v",
				peer.WorkerID, peer.VirtualIP, err)
			continue
		}

		conn.Close()
		log.Printf("[Multinode Executor] Successfully connected to peer %s (%s)",
			peer.WorkerID, peer.VirtualIP)
	}

	return nil
}

// executeWithPeers executes task with coordination across peer workers
func (e *MultinodeExecutor) executeWithPeers(ctx context.Context, task *Task, peers []*vpn.PeerInfo) (*TaskResult, error) {
	log.Printf("[Multinode Executor] Executing task %s with %d peers", task.ID, len(peers))

	// For now, execute locally and log peer information
	// In a full implementation, this would:
	// 1. Distribute subtasks to peers via VPN
	// 2. Collect results from all peers
	// 3. Aggregate results

	log.Printf("[Multinode Executor] Available peers:")
	for _, peer := range peers {
		log.Printf("  - Worker %s at %s (hostname: %s)",
			peer.WorkerID, peer.VirtualIP, peer.Hostname)
	}

	// Execute local portion
	result, err := e.localExecutor.Execute(ctx, task)
	if err != nil {
		return nil, fmt.Errorf("local execution failed: %w", err)
	}

	log.Printf("[Multinode Executor] Task %s completed successfully", task.ID)
	return result, nil
}

// DialPeer creates a connection to a peer worker
func (e *MultinodeExecutor) DialPeer(ctx context.Context, peerIP string, port int) (net.Conn, error) {
	tsnetClient := e.vpnManager.GetTsnetClient()
	if tsnetClient == nil {
		return nil, fmt.Errorf("VPN client not available")
	}

	address := fmt.Sprintf("%s:%d", peerIP, port)
	return tsnetClient.DialContext(ctx, "tcp", address)
}

// GetPeerList returns list of available peers for current task
func (e *MultinodeExecutor) GetPeerList(taskID string) ([]*vpn.PeerInfo, error) {
	return e.vpnManager.GetTaskPeers(taskID)
}

package vpn

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// Manager handles VPN lifecycle and communication with Nodepool
type Manager struct {
	tsnetClient    *TsnetClient
	nodepoolAddr   string
	nodepoolClient NodepoolVPNClient
	workerID       string
	taskID         string
	mu             sync.RWMutex
	registered     bool
	heartbeatStop  chan struct{}
	ctx            context.Context
	cancel         context.CancelFunc
}

// ManagerConfig holds configuration for VPN manager
type ManagerConfig struct {
	NodepoolAddr string
	WorkerID     string
	Hostname     string
	StateDir     string
}

// NodepoolVPNClient interface for communicating with Nodepool VPN service
type NodepoolVPNClient interface {
	RegisterVPN(ctx context.Context, req *VPNRegisterRequest) (*VPNRegisterResponse, error)
	UnregisterVPN(ctx context.Context, req *VPNUnregisterRequest) (*VPNUnregisterResponse, error)
	GetTaskPeers(ctx context.Context, req *TaskPeersRequest) (*TaskPeersResponse, error)
	SendHeartbeat(ctx context.Context, req *VPNHeartbeatRequest) (*VPNHeartbeatResponse, error)
}

// VPN message types (these should match the proto definitions)
type VPNRegisterRequest struct {
	WorkerID string
	Hostname string
}

type VPNRegisterResponse struct {
	Success  bool
	AuthKey  string
	ServerURL string
	Message  string
}

type VPNUnregisterRequest struct {
	WorkerID string
}

type VPNUnregisterResponse struct {
	Success bool
	Message string
}

type TaskPeersRequest struct {
	TaskID string
}

type TaskPeersResponse struct {
	Peers []*PeerInfo
}

type PeerInfo struct {
	WorkerID string
	Hostname string
	VirtualIP string
}

type VPNHeartbeatRequest struct {
	WorkerID  string
	VirtualIP string
	Status    string
}

type VPNHeartbeatResponse struct {
	Success bool
	Message string
}

// NewManager creates a new VPN manager
func NewManager(cfg *ManagerConfig) (*Manager, error) {
	if cfg.NodepoolAddr == "" {
		return nil, fmt.Errorf("nodepool address is required")
	}
	if cfg.WorkerID == "" {
		return nil, fmt.Errorf("worker ID is required")
	}

	ctx, cancel := context.WithCancel(context.Background())

	m := &Manager{
		nodepoolAddr:  cfg.NodepoolAddr,
		workerID:      cfg.WorkerID,
		heartbeatStop: make(chan struct{}),
		ctx:           ctx,
		cancel:        cancel,
	}

	return m, nil
}

// Start initializes VPN connection
func (m *Manager) Start() error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if m.registered {
		return fmt.Errorf("VPN already started")
	}

	log.Printf("[VPN Manager] Starting VPN for worker %s", m.workerID)

	// Connect to Nodepool
	conn, err := grpc.Dial(m.nodepoolAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return fmt.Errorf("failed to connect to nodepool: %w", err)
	}

	// Create VPN client (this would use the actual generated gRPC client)
	// For now, we'll use a mock implementation
	m.nodepoolClient = &mockNodepoolClient{}

	// Register with Nodepool VPN service
	registerReq := &VPNRegisterRequest{
		WorkerID: m.workerID,
		Hostname: fmt.Sprintf("worker-%s", m.workerID),
	}

	registerResp, err := m.nodepoolClient.RegisterVPN(m.ctx, registerReq)
	if err != nil {
		conn.Close()
		return fmt.Errorf("failed to register VPN: %w", err)
	}

	if !registerResp.Success {
		conn.Close()
		return fmt.Errorf("VPN registration failed: %s", registerResp.Message)
	}

	log.Printf("[VPN Manager] Registered with Nodepool, auth key received")

	// Initialize tsnet client
	tsnetCfg := &TsnetConfig{
		Hostname:   fmt.Sprintf("worker-%s", m.workerID),
		AuthKey:    registerResp.AuthKey,
		ControlURL: registerResp.ServerURL,
		StateDir:   fmt.Sprintf("/tmp/hivemind-vpn/%s", m.workerID),
		Ephemeral:  true,
	}

	tsnetClient, err := NewTsnetClient(tsnetCfg)
	if err != nil {
		conn.Close()
		return fmt.Errorf("failed to create tsnet client: %w", err)
	}

	// Start tsnet client
	if err := tsnetClient.Start(); err != nil {
		conn.Close()
		return fmt.Errorf("failed to start tsnet client: %w", err)
	}

	m.tsnetClient = tsnetClient
	m.registered = true

	// Start heartbeat goroutine
	go m.heartbeatLoop()

	log.Printf("[VPN Manager] VPN started successfully, local IP: %s", tsnetClient.GetLocalIP())

	return nil
}

// Stop gracefully shuts down VPN
func (m *Manager) Stop() error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if !m.registered {
		return nil
	}

	log.Printf("[VPN Manager] Stopping VPN for worker %s", m.workerID)

	// Stop heartbeat
	close(m.heartbeatStop)

	// Unregister from Nodepool
	if m.nodepoolClient != nil {
		unregisterReq := &VPNUnregisterRequest{
			WorkerID: m.workerID,
		}
		_, err := m.nodepoolClient.UnregisterVPN(m.ctx, unregisterReq)
		if err != nil {
			log.Printf("[VPN Manager] Failed to unregister: %v", err)
		}
	}

	// Stop tsnet client
	if m.tsnetClient != nil {
		if err := m.tsnetClient.Stop(); err != nil {
			log.Printf("[VPN Manager] Failed to stop tsnet client: %v", err)
		}
	}

	m.cancel()
	m.registered = false

	log.Printf("[VPN Manager] VPN stopped")

	return nil
}

// heartbeatLoop sends periodic heartbeats to Nodepool
func (m *Manager) heartbeatLoop() {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-m.heartbeatStop:
			return
		case <-ticker.C:
			m.sendHeartbeat()
		}
	}
}

// sendHeartbeat sends a heartbeat to Nodepool
func (m *Manager) sendHeartbeat() {
	m.mu.RLock()
	client := m.nodepoolClient
	tsnet := m.tsnetClient
	m.mu.RUnlock()

	if client == nil || tsnet == nil {
		return
	}

	req := &VPNHeartbeatRequest{
		WorkerID:  m.workerID,
		VirtualIP: tsnet.GetLocalIP(),
		Status:    "active",
	}

	ctx, cancel := context.WithTimeout(m.ctx, 5*time.Second)
	defer cancel()

	resp, err := client.SendHeartbeat(ctx, req)
	if err != nil {
		log.Printf("[VPN Manager] Heartbeat failed: %v", err)
		return
	}

	if !resp.Success {
		log.Printf("[VPN Manager] Heartbeat rejected: %s", resp.Message)
	}
}

// GetTaskPeers retrieves peer information for a task
func (m *Manager) GetTaskPeers(taskID string) ([]*PeerInfo, error) {
	m.mu.RLock()
	client := m.nodepoolClient
	m.mu.RUnlock()

	if client == nil {
		return nil, fmt.Errorf("VPN not initialized")
	}

	req := &TaskPeersRequest{
		TaskID: taskID,
	}

	ctx, cancel := context.WithTimeout(m.ctx, 10*time.Second)
	defer cancel()

	resp, err := client.GetTaskPeers(ctx, req)
	if err != nil {
		return nil, fmt.Errorf("failed to get task peers: %w", err)
	}

	return resp.Peers, nil
}

// GetLocalIP returns the VPN local IP
func (m *Manager) GetLocalIP() string {
	m.mu.RLock()
	defer m.mu.RUnlock()

	if m.tsnetClient == nil {
		return ""
	}

	return m.tsnetClient.GetLocalIP()
}

// IsConnected returns whether VPN is connected
func (m *Manager) IsConnected() bool {
	m.mu.RLock()
	defer m.mu.RUnlock()

	return m.registered && m.tsnetClient != nil && m.tsnetClient.IsConnected()
}

// Dial creates a connection to a peer
func (m *Manager) Dial(network, address string) (interface{}, error) {
	m.mu.RLock()
	tsnet := m.tsnetClient
	m.mu.RUnlock()

	if tsnet == nil {
		return nil, fmt.Errorf("VPN not initialized")
	}

	return tsnet.Dial(network, address)
}


package vpn

import (
	"context"
	"fmt"
	"log"
	"net"
	"os"
	"path/filepath"
	"sync"
	"time"
)

// TsnetClient wraps tsnet.Server and provides VPN connectivity
// Note: This is a stub implementation. To use real tsnet, import "tailscale.com/tsnet"
type TsnetClient struct {
	hostname    string
	authKey     string
	controlURL  string
	stateDir    string
	mu          sync.RWMutex
	connected   bool
	localIP     string
	ctx         context.Context
	cancel      context.CancelFunc
}

// TsnetConfig holds configuration for tsnet client
type TsnetConfig struct {
	Hostname   string // Worker's unique hostname
	AuthKey    string // Pre-auth key from Headscale
	ControlURL string // Headscale server URL
	StateDir   string // Directory to store tsnet state
	Ephemeral  bool   // Whether this is an ephemeral node
}

// NewTsnetClient creates a new tsnet VPN client
func NewTsnetClient(cfg *TsnetConfig) (*TsnetClient, error) {
	if cfg.Hostname == "" {
		return nil, fmt.Errorf("hostname is required")
	}
	if cfg.ControlURL == "" {
		return nil, fmt.Errorf("control URL is required")
	}

	// Create state directory if not exists
	stateDir := cfg.StateDir
	if stateDir == "" {
		stateDir = filepath.Join(os.TempDir(), "hivemind-vpn", cfg.Hostname)
	}
	if err := os.MkdirAll(stateDir, 0700); err != nil {
		return nil, fmt.Errorf("failed to create state dir: %w", err)
	}

	ctx, cancel := context.WithCancel(context.Background())

	client := &TsnetClient{
		hostname:   cfg.Hostname,
		authKey:    cfg.AuthKey,
		controlURL: cfg.ControlURL,
		stateDir:   stateDir,
		ctx:        ctx,
		cancel:     cancel,
	}

	return client, nil
}

// Start initializes and starts the tsnet client
// Note: This is a stub implementation for testing without real tsnet
func (c *TsnetClient) Start() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.connected {
		return fmt.Errorf("client already started")
	}

	log.Printf("[VPN] Starting tsnet client: hostname=%s, control=%s", c.hostname, c.controlURL)

	// TODO: Replace with real tsnet implementation
	// For now, simulate connection
	log.Printf("[VPN] WARNING: Using stub tsnet implementation. Install tailscale.com/tsnet for real VPN.")

	// Simulate connection delay
	time.Sleep(100 * time.Millisecond)

	// Assign a mock local IP
	c.localIP = "100.64.0.1"
	c.connected = true

	log.Printf("[VPN] Connected successfully (stub), local IP: %s", c.localIP)

	return nil
}

// Stop gracefully shuts down the tsnet client
func (c *TsnetClient) Stop() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if !c.connected {
		return nil
	}

	log.Printf("[VPN] Stopping tsnet client")

	c.cancel()
	c.connected = false

	log.Printf("[VPN] Stopped successfully")

	return nil
}

// Listen creates a listener on the VPN network
func (c *TsnetClient) Listen(network, address string) (net.Listener, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if !c.connected {
		return nil, fmt.Errorf("VPN not connected")
	}

	// TODO: Replace with real tsnet listener
	return net.Listen(network, address)
}

// Dial creates a connection to a peer on the VPN network
func (c *TsnetClient) Dial(network, address string) (net.Conn, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if !c.connected {
		return nil, fmt.Errorf("VPN not connected")
	}

	// TODO: Replace with real tsnet dial
	return net.Dial(network, address)
}

// DialContext creates a connection with context
func (c *TsnetClient) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if !c.connected {
		return nil, fmt.Errorf("VPN not connected")
	}

	// TODO: Replace with real tsnet dial
	var d net.Dialer
	return d.DialContext(ctx, network, address)
}

// GetLocalIP returns the VPN local IP address
func (c *TsnetClient) GetLocalIP() string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.localIP
}

// IsConnected returns whether the VPN is connected
func (c *TsnetClient) IsConnected() bool {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.connected
}

// GetPeerStatus returns status of all peers
func (c *TsnetClient) GetPeerStatus() (map[string]string, error) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if !c.connected {
		return nil, fmt.Errorf("VPN not connected")
	}

	// TODO: Replace with real tsnet peer status
	// Return mock data for testing
	peers := make(map[string]string)
	return peers, nil
}


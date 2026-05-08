package vpn

import (
	"testing"
	"time"
)

func TestNewTsnetClient(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
		Ephemeral:  true,
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	if client.hostname != cfg.Hostname {
		t.Errorf("Expected hostname %s, got %s", cfg.Hostname, client.hostname)
	}

	if client.authKey != cfg.AuthKey {
		t.Errorf("Expected authKey %s, got %s", cfg.AuthKey, client.authKey)
	}

	if client.controlURL != cfg.ControlURL {
		t.Errorf("Expected controlURL %s, got %s", cfg.ControlURL, client.controlURL)
	}
}

func TestTsnetClientValidation(t *testing.T) {
	tests := []struct {
		name    string
		cfg     *TsnetConfig
		wantErr bool
	}{
		{
			name: "valid config",
			cfg: &TsnetConfig{
				Hostname:   "test-worker",
				ControlURL: "http://localhost:8080",
			},
			wantErr: false,
		},
		{
			name: "missing hostname",
			cfg: &TsnetConfig{
				ControlURL: "http://localhost:8080",
			},
			wantErr: true,
		},
		{
			name: "missing control URL",
			cfg: &TsnetConfig{
				Hostname: "test-worker",
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := NewTsnetClient(tt.cfg)
			if (err != nil) != tt.wantErr {
				t.Errorf("NewTsnetClient() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestTsnetClientStartStop(t *testing.T) {
	t.Skip("Skipping integration test - requires Headscale server")

	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
		Ephemeral:  true,
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Start client
	if err := client.Start(); err != nil {
		t.Fatalf("Failed to start tsnet client: %v", err)
	}

	// Verify connected
	if !client.IsConnected() {
		t.Error("Client should be connected after Start()")
	}

	// Check local IP
	localIP := client.GetLocalIP()
	if localIP == "" {
		t.Error("Local IP should not be empty after connection")
	}

	// Wait a bit
	time.Sleep(2 * time.Second)

	// Stop client
	if err := client.Stop(); err != nil {
		t.Fatalf("Failed to stop tsnet client: %v", err)
	}

	// Verify disconnected
	if client.IsConnected() {
		t.Error("Client should not be connected after Stop()")
	}
}

func TestTsnetClientGetLocalIP(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Before connection, IP should be empty
	if client.GetLocalIP() != "" {
		t.Error("Local IP should be empty before connection")
	}

	// Simulate connection
	client.connected = true
	client.localIP = "100.64.0.1"

	if client.GetLocalIP() != "100.64.0.1" {
		t.Errorf("Expected local IP 100.64.0.1, got %s", client.GetLocalIP())
	}
}

func TestTsnetClientIsConnected(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Initially not connected
	if client.IsConnected() {
		t.Error("Client should not be connected initially")
	}

	// Simulate connection
	client.connected = true

	if !client.IsConnected() {
		t.Error("Client should be connected after setting connected flag")
	}
}

func TestTsnetClientDialBeforeConnect(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Try to dial before connecting
	_, err = client.Dial("tcp", "100.64.0.2:8080")
	if err == nil {
		t.Error("Dial should fail when not connected")
	}
}

func TestTsnetClientListenBeforeConnect(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Try to listen before connecting
	_, err = client.Listen("tcp", ":8080")
	if err == nil {
		t.Error("Listen should fail when not connected")
	}
}

func TestTsnetClientConcurrency(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "/tmp/test-tsnet",
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Test concurrent access to read methods
	done := make(chan bool)
	for i := 0; i < 10; i++ {
		go func() {
			_ = client.GetLocalIP()
			_ = client.IsConnected()
			done <- true
		}()
	}

	for i := 0; i < 10; i++ {
		<-done
	}
}

func TestTsnetClientStateDir(t *testing.T) {
	cfg := &TsnetConfig{
		Hostname:   "test-worker",
		AuthKey:    "test-auth-key",
		ControlURL: "http://localhost:8080",
		StateDir:   "",
	}

	client, err := NewTsnetClient(cfg)
	if err != nil {
		t.Fatalf("Failed to create tsnet client: %v", err)
	}

	// Should have default state dir
	if client.stateDir == "" {
		t.Error("State dir should not be empty")
	}
}

package config

import (
	"testing"
)

func TestGetEnv(t *testing.T) {
	// Test with existing env var
	t.Setenv("TEST_VAR", "test_value")
	result := GetEnv("TEST_VAR", "fallback")
	if result != "test_value" {
		t.Errorf("Expected 'test_value', got '%s'", result)
	}

	// Test with non-existing env var
	result = GetEnv("NON_EXISTING_VAR", "fallback")
	if result != "fallback" {
		t.Errorf("Expected 'fallback', got '%s'", result)
	}
}

func TestGetEnvBool(t *testing.T) {
	tests := []struct {
		name     string
		envValue string
		fallback bool
		expected bool
	}{
		{"true string", "true", false, true},
		{"false string", "false", true, false},
		{"1 string", "1", false, true},
		{"0 string", "0", true, false},
		{"invalid string", "invalid", true, true},
		{"empty string", "", false, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if tt.envValue != "" {
				t.Setenv("TEST_BOOL_VAR", tt.envValue)
				result := GetEnvBool("TEST_BOOL_VAR", tt.fallback)
				if result != tt.expected {
					t.Errorf("Expected %v, got %v", tt.expected, result)
				}
			} else {
				result := GetEnvBool("NON_EXISTING_BOOL_VAR", tt.fallback)
				if result != tt.fallback {
					t.Errorf("Expected %v, got %v", tt.fallback, result)
				}
			}
		})
	}
}

func TestNodepoolAddr(t *testing.T) {
	// Test default value
	addr := NodepoolAddr()
	if addr != "localhost:50051" {
		t.Errorf("Expected default 'localhost:50051', got '%s'", addr)
	}

	// Test with env var
	t.Setenv("NODEPOOL_ADDR", "nodepool.example.com:50051")
	addr = NodepoolAddr()
	if addr != "nodepool.example.com:50051" {
		t.Errorf("Expected 'nodepool.example.com:50051', got '%s'", addr)
	}
}

func TestWorkerGRPCPort(t *testing.T) {
	// Test default value
	port := WorkerGRPCPort()
	if port != ":50052" {
		t.Errorf("Expected default ':50052', got '%s'", port)
	}

	// Test with env var
	t.Setenv("WORKER_GRPC_PORT", ":8080")
	port = WorkerGRPCPort()
	if port != ":8080" {
		t.Errorf("Expected ':8080', got '%s'", port)
	}
}

func TestWorkerID(t *testing.T) {
	// Test default value (empty)
	id := WorkerID()
	if id != "" {
		t.Errorf("Expected empty string, got '%s'", id)
	}

	// Test with env var
	t.Setenv("WORKER_ID", "worker-123")
	id = WorkerID()
	if id != "worker-123" {
		t.Errorf("Expected 'worker-123', got '%s'", id)
	}
}

func TestVPNEnabled(t *testing.T) {
	// Test default value (true)
	enabled := VPNEnabled()
	if !enabled {
		t.Error("Expected VPN to be enabled by default")
	}

	// Test with env var
	t.Setenv("VPN_ENABLED", "false")
	enabled = VPNEnabled()
	if enabled {
		t.Error("Expected VPN to be disabled")
	}
}

func TestVPNStateDir(t *testing.T) {
	// Test default value
	dir := VPNStateDir()
	if dir != "/tmp/hivemind-vpn" {
		t.Errorf("Expected default '/tmp/hivemind-vpn', got '%s'", dir)
	}

	// Test with env var
	t.Setenv("VPN_STATE_DIR", "/custom/vpn/dir")
	dir = VPNStateDir()
	if dir != "/custom/vpn/dir" {
		t.Errorf("Expected '/custom/vpn/dir', got '%s'", dir)
	}
}

func TestVPNHostname(t *testing.T) {
	// Test default value (empty)
	hostname := VPNHostname()
	if hostname != "" {
		t.Errorf("Expected empty string, got '%s'", hostname)
	}

	// Test with env var
	t.Setenv("VPN_HOSTNAME", "my-worker")
	hostname = VPNHostname()
	if hostname != "my-worker" {
		t.Errorf("Expected 'my-worker', got '%s'", hostname)
	}
}

func TestLoadConfig(t *testing.T) {
	// Set up environment
	t.Setenv("NODEPOOL_ADDR", "nodepool.test:50051")
	t.Setenv("WORKER_GRPC_PORT", ":9090")
	t.Setenv("WORKER_ID", "test-worker-1")
	t.Setenv("VPN_ENABLED", "true")
	t.Setenv("VPN_STATE_DIR", "/test/vpn")
	t.Setenv("VPN_HOSTNAME", "test-hostname")

	cfg := LoadConfig()

	if cfg.NodepoolAddr != "nodepool.test:50051" {
		t.Errorf("Expected NodepoolAddr 'nodepool.test:50051', got '%s'", cfg.NodepoolAddr)
	}

	if cfg.WorkerGRPCPort != ":9090" {
		t.Errorf("Expected WorkerGRPCPort ':9090', got '%s'", cfg.WorkerGRPCPort)
	}

	if cfg.WorkerID != "test-worker-1" {
		t.Errorf("Expected WorkerID 'test-worker-1', got '%s'", cfg.WorkerID)
	}

	if !cfg.VPN.Enabled {
		t.Error("Expected VPN to be enabled")
	}

	if cfg.VPN.StateDir != "/test/vpn" {
		t.Errorf("Expected VPN StateDir '/test/vpn', got '%s'", cfg.VPN.StateDir)
	}

	if cfg.VPN.Hostname != "test-hostname" {
		t.Errorf("Expected VPN Hostname 'test-hostname', got '%s'", cfg.VPN.Hostname)
	}
}

func TestLoadConfigDefaultHostname(t *testing.T) {
	// Set worker ID but not hostname
	t.Setenv("WORKER_ID", "worker-123")

	cfg := LoadConfig()

	// Should generate hostname from worker ID
	expectedHostname := "worker-worker-123"
	if cfg.VPN.Hostname != expectedHostname {
		t.Errorf("Expected VPN Hostname '%s', got '%s'", expectedHostname, cfg.VPN.Hostname)
	}
}

func TestLoadConfigNoWorkerID(t *testing.T) {
	cfg := LoadConfig()

	// Should have empty worker ID and hostname
	if cfg.WorkerID != "" {
		t.Errorf("Expected empty WorkerID, got '%s'", cfg.WorkerID)
	}

	if cfg.VPN.Hostname != "" {
		t.Errorf("Expected empty VPN Hostname, got '%s'", cfg.VPN.Hostname)
	}
}

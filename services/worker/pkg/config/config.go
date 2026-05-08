package config

import (
	"os"
	"strconv"
)

// Config holds all worker configuration
type Config struct {
	NodepoolAddr string
	WorkerGRPCPort string
	WorkerID     string
	VPN          VPNConfig
}

// VPNConfig holds VPN-related configuration
type VPNConfig struct {
	Enabled   bool
	StateDir  string
	Hostname  string
}

func GetEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func GetEnvBool(key string, fallback bool) bool {
	if v := os.Getenv(key); v != "" {
		b, err := strconv.ParseBool(v)
		if err == nil {
			return b
		}
	}
	return fallback
}

func NodepoolAddr() string {
	return GetEnv("NODEPOOL_ADDR", "localhost:50051")
}

func WorkerGRPCPort() string {
	return GetEnv("WORKER_GRPC_PORT", ":50052")
}

func WorkerID() string {
	return GetEnv("WORKER_ID", "")
}

// VPNEnabled returns whether VPN is enabled
func VPNEnabled() bool {
	return GetEnvBool("VPN_ENABLED", true)
}

// VPNStateDir returns the VPN state directory
func VPNStateDir() string {
	return GetEnv("VPN_STATE_DIR", "/tmp/hivemind-vpn")
}

// VPNHostname returns the VPN hostname
func VPNHostname() string {
	return GetEnv("VPN_HOSTNAME", "")
}

// LoadConfig loads configuration from environment
func LoadConfig() *Config {
	workerID := WorkerID()
	hostname := VPNHostname()
	if hostname == "" && workerID != "" {
		hostname = "worker-" + workerID
	}

	return &Config{
		NodepoolAddr:   NodepoolAddr(),
		WorkerGRPCPort: WorkerGRPCPort(),
		WorkerID:       workerID,
		VPN: VPNConfig{
			Enabled:  VPNEnabled(),
			StateDir: VPNStateDir(),
			Hostname: hostname,
		},
	}
}

package config

// config.go: configuration loader for nodepool service

import "os"

func GetEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func GRPCPort() string {
	return GetEnv("NODEPOOL_GRPC_PORT", ":50051")
}

package config

import "os"

func GetEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func NodepoolAddr() string {
	return GetEnv("NODEPOOL_ADDR", "localhost:50051")
}

func WorkerGRPCPort() string {
	return GetEnv("WORKER_GRPC_PORT", ":50052")
}

package main

import (
	"log"

	"hivemind/services/nodepool/pkg/server"
)

func main() {
	// Start the nodepool gRPC server (handler implementations in pkg/handlers)
	if err := server.Start(); err != nil {
		log.Fatalf("server failed: %v", err)
	}
}

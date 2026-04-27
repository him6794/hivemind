package main

import (
	"log"

	"github.com/yourorg/hivemind/services/worker/pkg/server"
)

func main() {
	if err := server.Start(); err != nil {
		log.Fatalf("worker server failed: %v", err)
	}
}

package main

import (
	"log"

	"hivemind/services/worker/pkg/server"
)

func main() {
	if err := server.Start(); err != nil {
		log.Fatalf("worker server failed: %v", err)
	}
}

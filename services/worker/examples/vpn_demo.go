package main

import (
	"context"
	"flag"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"hivemind/services/worker/pkg/config"
	"hivemind/services/worker/pkg/executor"
	"hivemind/services/worker/pkg/handlers"
	"hivemind/services/worker/pkg/vpn"
)

// Example demonstrates how to use the VPN integration in a Worker

func main() {
	// Parse command line flags
	workerID := flag.String("worker-id", "worker-001", "Unique worker ID")
	nodepoolAddr := flag.String("nodepool", "localhost:50051", "Nodepool address")
	enableVPN := flag.Bool("vpn", true, "Enable VPN")
	flag.Parse()

	// Set environment variables
	os.Setenv("WORKER_ID", *workerID)
	os.Setenv("NODEPOOL_ADDR", *nodepoolAddr)
	os.Setenv("VPN_ENABLED", "true")
	if !*enableVPN {
		os.Setenv("VPN_ENABLED", "false")
	}

	log.Printf("Starting Worker %s", *workerID)

	// Load configuration
	cfg := config.LoadConfig()

	// Create VPN manager if enabled
	var vpnMgr *vpn.Manager
	if cfg.VPN.Enabled {
		vpnCfg := &vpn.ManagerConfig{
			NodepoolAddr: cfg.NodepoolAddr,
			WorkerID:     cfg.WorkerID,
			Hostname:     cfg.VPN.Hostname,
			StateDir:     cfg.VPN.StateDir,
		}

		var err error
		vpnMgr, err = vpn.NewManager(vpnCfg)
		if err != nil {
			log.Fatalf("Failed to create VPN manager: %v", err)
		}
		log.Printf("VPN manager created")
	}

	// Create registration handler
	registration := handlers.NewWorkerRegistration(cfg.WorkerID, vpnMgr)

	// Register worker and start VPN
	ctx := context.Background()
	if err := registration.Register(ctx); err != nil {
		log.Fatalf("Failed to register worker: %v", err)
	}
	log.Printf("Worker registered successfully")

	// If VPN is enabled, demonstrate VPN features
	if vpnMgr != nil && vpnMgr.IsRegistered() {
		demonstrateVPNFeatures(vpnMgr)
	}

	// Wait for shutdown signal
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	log.Printf("Worker running. Press Ctrl+C to stop.")
	<-sigChan

	log.Printf("Shutting down...")

	// Unregister worker and stop VPN
	if err := registration.Unregister(ctx); err != nil {
		log.Printf("Failed to unregister: %v", err)
	}

	log.Printf("Worker stopped")
}

func demonstrateVPNFeatures(vpnMgr *vpn.Manager) {
	log.Printf("=== VPN Features Demo ===")

	// Get tsnet client
	tsnetClient := vpnMgr.GetTsnetClient()
	if tsnetClient == nil {
		log.Printf("VPN client not available")
		return
	}

	// Show local IP
	localIP := tsnetClient.GetLocalIP()
	log.Printf("Local VPN IP: %s", localIP)

	// Show connection status
	if tsnetClient.IsConnected() {
		log.Printf("VPN Status: Connected")
	} else {
		log.Printf("VPN Status: Disconnected")
	}

	// Get peer status
	peers, err := tsnetClient.GetPeerStatus()
	if err != nil {
		log.Printf("Failed to get peer status: %v", err)
	} else {
		log.Printf("Connected peers: %d", len(peers))
		for hostname, ip := range peers {
			log.Printf("  - %s: %s", hostname, ip)
		}
	}

	// Demonstrate task peer discovery
	go demonstrateTaskExecution(vpnMgr)
}

func demonstrateTaskExecution(vpnMgr *vpn.Manager) {
	// Wait a bit for other workers to join
	time.Sleep(5 * time.Second)

	log.Printf("=== Task Execution Demo ===")

	// Set a task ID
	taskID := "demo-task-001"
	vpnMgr.SetTaskID(taskID)
	log.Printf("Set task ID: %s", taskID)

	// Get peers for this task
	peers, err := vpnMgr.GetTaskPeers(taskID)
	if err != nil {
		log.Printf("Failed to get task peers: %v", err)
		return
	}

	log.Printf("Task peers: %d", len(peers))
	for _, peer := range peers {
		log.Printf("  - Worker %s (%s) at %s", peer.WorkerID, peer.Hostname, peer.VirtualIP)
	}

	// Create multinode executor
	localExec := &demoExecutor{}
	multinodeExec := executor.NewMultinodeExecutor(vpnMgr, localExec)

	// Create a demo task
	task := &executor.Task{
		ID:            taskID,
		Type:          "demo-compute",
		Payload:       []byte("demo payload"),
		RequiresPeers: len(peers) > 0,
	}

	// Execute task
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	log.Printf("Executing task %s...", taskID)
	result, err := multinodeExec.Execute(ctx, task)
	if err != nil {
		log.Printf("Task execution failed: %v", err)
		return
	}

	if result.Success {
		log.Printf("Task completed successfully: %s", result.Stdout)
	} else {
		log.Printf("Task failed: %s", result.Error)
	}
}

// demoExecutor is a simple executor for demonstration
type demoExecutor struct{}

func (e *demoExecutor) Execute(ctx context.Context, task *executor.Task) (*executor.TaskResult, error) {
	log.Printf("Executing task %s locally", task.ID)

	// Simulate some work
	time.Sleep(2 * time.Second)

	return &executor.TaskResult{
		Success: true,
		Stdout:  "Demo task completed",
	}, nil
}

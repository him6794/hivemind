package server

import (
	"context"
	"fmt"
	"log"
	"net"
	"os"
	"os/signal"
	"syscall"

	"google.golang.org/grpc"

	"hivemind/services/worker/pkg/config"
	"hivemind/services/worker/pkg/handlers"
	"hivemind/services/worker/pkg/vpn"
)

// Server represents the worker server
type Server struct {
	grpcServer   *grpc.Server
	vpnManager   *vpn.Manager
	registration *handlers.WorkerRegistration
	config       *config.Config
	ctx          context.Context
	cancel       context.CancelFunc
}

// NewServer creates a new worker server
func NewServer(cfg *config.Config) (*Server, error) {
	ctx, cancel := context.WithCancel(context.Background())

	s := &Server{
		config: cfg,
		ctx:    ctx,
		cancel: cancel,
	}

	// Initialize VPN manager if enabled
	if cfg.VPN.Enabled {
		vpnCfg := &vpn.ManagerConfig{
			NodepoolAddr: cfg.NodepoolAddr,
			WorkerID:     cfg.WorkerID,
			Hostname:     cfg.VPN.Hostname,
			StateDir:     cfg.VPN.StateDir,
		}

		vpnMgr, err := vpn.NewManager(vpnCfg)
		if err != nil {
			cancel()
			return nil, fmt.Errorf("failed to create VPN manager: %w", err)
		}
		s.vpnManager = vpnMgr
		log.Printf("[Server] VPN manager initialized")
	}

	// Initialize registration handler
	s.registration = handlers.NewWorkerRegistration(cfg.WorkerID, s.vpnManager)

	return s, nil
}

// Start starts the worker gRPC server and registers with Nodepool
func (s *Server) Start() error {
	// Register worker and join VPN
	if err := s.registration.Register(s.ctx); err != nil {
		return fmt.Errorf("failed to register worker: %w", err)
	}

	// Start gRPC server
	lis, err := net.Listen("tcp", s.config.WorkerGRPCPort)
	if err != nil {
		s.registration.Unregister(s.ctx)
		return fmt.Errorf("listen: %w", err)
	}

	s.grpcServer = grpc.NewServer()

	// TODO: register generated protobuf services / clients used by worker
	// e.g. nodepool pb clients for ReportStatus / RegisterWorkerNode

	go func() {
		log.Printf("[Server] Starting gRPC server on %s", s.config.WorkerGRPCPort)
		if err := s.grpcServer.Serve(lis); err != nil {
			log.Printf("[Server] gRPC serve error: %v", err)
		}
	}()

	log.Printf("[Server] Worker started successfully")

	// Wait for shutdown signal
	s.waitForShutdown()

	return nil
}

// waitForShutdown waits for interrupt signal and performs graceful shutdown
func (s *Server) waitForShutdown() {
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	select {
	case sig := <-sigChan:
		log.Printf("[Server] Received signal: %v", sig)
	case <-s.ctx.Done():
		log.Printf("[Server] Context cancelled")
	}

	s.Shutdown()
}

// Shutdown gracefully shuts down the server
func (s *Server) Shutdown() {
	log.Printf("[Server] Shutting down...")

	// Stop gRPC server
	if s.grpcServer != nil {
		s.grpcServer.GracefulStop()
	}

	// Unregister worker and leave VPN
	if s.registration != nil {
		if err := s.registration.Unregister(s.ctx); err != nil {
			log.Printf("[Server] Failed to unregister: %v", err)
		}
	}

	s.cancel()
	log.Printf("[Server] Shutdown complete")
}

// GetVPNManager returns the VPN manager
func (s *Server) GetVPNManager() *vpn.Manager {
	return s.vpnManager
}

// Start is the legacy function for compatibility
func Start() error {
	cfg := config.LoadConfig()

	server, err := NewServer(cfg)
	if err != nil {
		return fmt.Errorf("failed to create server: %w", err)
	}

	return server.Start()
}

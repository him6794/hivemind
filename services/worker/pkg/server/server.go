package server

import (
	"context"
	"fmt"
	"net"

	"google.golang.org/grpc"
)

// Start starts the worker gRPC server. Handlers live in pkg/handlers.
func Start() error {
	lis, err := net.Listen("tcp", ":50052")
	if err != nil {
		return fmt.Errorf("listen: %w", err)
	}

	s := grpc.NewServer()

	// TODO: register generated protobuf services / clients used by worker
	// e.g. nodepool pb clients for ReportStatus / RegisterWorkerNode

	go func() {
		if err := s.Serve(lis); err != nil {
			fmt.Printf("worker gRPC serve error: %v\n", err)
		}
	}()

	select {
	case <-context.Background().Done():
	}

	s.GracefulStop()
	return nil
}

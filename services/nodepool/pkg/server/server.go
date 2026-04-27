package server

import (
	"context"
	"fmt"
	"net"

	"google.golang.org/grpc"
)

// Start starts the nodepool gRPC server. Handlers live in pkg/handlers.
func Start() error {
	lis, err := net.Listen("tcp", ":50051")
	if err != nil {
		return fmt.Errorf("listen: %w", err)
	}

	s := grpc.NewServer()

	// TODO: register generated protobuf services, e.g.:
	// nodepool.RegisterNodeManagerServiceServer(s, handlers.NewNodeManager())

	go func() {
		if err := s.Serve(lis); err != nil {
			// log from main
			fmt.Printf("gRPC serve error: %v\n", err)
		}
	}()

	// block until context cancelled or signal received in future
	// for scaffold we'll just block here
	select {
	case <-context.Background().Done():
	}

	s.GracefulStop()
	return nil
}

package vpn

import (
	"context"
)

// mockNodepoolClient is a mock implementation for testing
type mockNodepoolClient struct{}

func (m *mockNodepoolClient) RegisterVPN(ctx context.Context, req *VPNRegisterRequest) (*VPNRegisterResponse, error) {
	return &VPNRegisterResponse{
		Success:   true,
		AuthKey:   "mock-auth-key-" + req.WorkerID,
		ServerURL: "http://localhost:8080",
		Message:   "Mock registration successful",
	}, nil
}

func (m *mockNodepoolClient) UnregisterVPN(ctx context.Context, req *VPNUnregisterRequest) (*VPNUnregisterResponse, error) {
	return &VPNUnregisterResponse{
		Success: true,
		Message: "Mock unregistration successful",
	}, nil
}

func (m *mockNodepoolClient) GetTaskPeers(ctx context.Context, req *TaskPeersRequest) (*TaskPeersResponse, error) {
	return &TaskPeersResponse{
		Peers: []*PeerInfo{
			{
				WorkerID:  "worker-2",
				Hostname:  "worker-2",
				VirtualIP: "100.64.0.2",
			},
		},
	}, nil
}

func (m *mockNodepoolClient) SendHeartbeat(ctx context.Context, req *VPNHeartbeatRequest) (*VPNHeartbeatResponse, error) {
	return &VPNHeartbeatResponse{
		Success: true,
		Message: "Heartbeat received",
	}, nil
}

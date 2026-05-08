package handler

import (
	"context"
	"testing"
	"time"

	"hivemind/services/nodepool/internal/vpn"
	"hivemind/services/nodepool/pb"
)

func setupTestVPNHandler(t *testing.T) *VPNHandler {
	config := &vpn.Config{
		ServerURL:      "http://localhost:8080",
		IPPrefix:       "100.64.0.0/10",
		BaseDomain:     "hivemind.local",
		EphemeralNodes: true,
		NodeExpiry:     24 * time.Hour,
		DBType:         "sqlite",
		DBPath:         "/tmp/test.db",
	}

	manager, err := vpn.NewHeadscaleManager(config)
	if err != nil {
		t.Fatalf("Failed to create VPN manager: %v", err)
	}

	return NewVPNHandler(manager)
}

func TestJoinVPN(t *testing.T) {
	handler := setupTestVPNHandler(t)
	ctx := context.Background()

	tests := []struct {
		name           string
		req            *pb.JoinVPNRequest
		wantSuccess    bool
		wantErrMessage string
	}{
		{
			name: "valid request",
			req: &pb.JoinVPNRequest{
				WorkerId:  "worker-1",
				Hostname:  "worker1.local",
				AuthToken: "test-token",
			},
			wantSuccess: true,
		},
		{
			name: "missing worker_id",
			req: &pb.JoinVPNRequest{
				Hostname:  "worker1.local",
				AuthToken: "test-token",
			},
			wantSuccess:    false,
			wantErrMessage: "worker_id is required",
		},
		{
			name: "missing hostname",
			req: &pb.JoinVPNRequest{
				WorkerId:  "worker-1",
				AuthToken: "test-token",
			},
			wantSuccess:    false,
			wantErrMessage: "hostname is required",
		},
		{
			name: "duplicate registration",
			req: &pb.JoinVPNRequest{
				WorkerId:  "worker-1",
				Hostname:  "worker1.local",
				AuthToken: "test-token",
			},
			wantSuccess: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			resp, err := handler.JoinVPN(ctx, tt.req)
			if err != nil {
				t.Fatalf("JoinVPN() error = %v", err)
			}

			if resp.Success != tt.wantSuccess {
				t.Errorf("JoinVPN() success = %v, want %v", resp.Success, tt.wantSuccess)
			}

			if !tt.wantSuccess && resp.StatusMessage != tt.wantErrMessage {
				t.Errorf("JoinVPN() status_message = %v, want %v", resp.StatusMessage, tt.wantErrMessage)
			}

			if tt.wantSuccess {
				if resp.VirtualIp == "" {
					t.Error("JoinVPN() virtual_ip is empty")
				}
				if resp.AuthKey == "" {
					t.Error("JoinVPN() auth_key is empty")
				}
				if resp.DerpMap == "" {
					t.Error("JoinVPN() derp_map is empty")
				}
			}
		})
	}
}

func TestGetTaskPeers(t *testing.T) {
	handler := setupTestVPNHandler(t)
	ctx := context.Background()

	// 註冊幾個 Worker
	_, err := handler.JoinVPN(ctx, &pb.JoinVPNRequest{
		WorkerId:  "worker-1",
		Hostname:  "worker1.local",
		AuthToken: "test-token",
	})
	if err != nil {
		t.Fatalf("Failed to join worker-1: %v", err)
	}

	_, err = handler.JoinVPN(ctx, &pb.JoinVPNRequest{
		WorkerId:  "worker-2",
		Hostname:  "worker2.local",
		AuthToken: "test-token",
	})
	if err != nil {
		t.Fatalf("Failed to join worker-2: %v", err)
	}

	// 分配 Worker 到任務
	handler.vpnManager.AssignWorkerToTask(ctx, "task-1", "worker-1")
	handler.vpnManager.AssignWorkerToTask(ctx, "task-1", "worker-2")

	tests := []struct {
		name           string
		req            *pb.GetTaskPeersRequest
		wantSuccess    bool
		wantPeerCount  int
		wantErrMessage string
	}{
		{
			name: "valid request",
			req: &pb.GetTaskPeersRequest{
				TaskId:    "task-1",
				WorkerId:  "worker-1",
				AuthToken: "test-token",
			},
			wantSuccess:   true,
			wantPeerCount: 1, // worker-2 (排除自己)
		},
		{
			name: "missing task_id",
			req: &pb.GetTaskPeersRequest{
				WorkerId:  "worker-1",
				AuthToken: "test-token",
			},
			wantSuccess:    false,
			wantErrMessage: "task_id is required",
		},
		{
			name: "missing worker_id",
			req: &pb.GetTaskPeersRequest{
				TaskId:    "task-1",
				AuthToken: "test-token",
			},
			wantSuccess:    false,
			wantErrMessage: "worker_id is required",
		},
		{
			name: "non-existing task",
			req: &pb.GetTaskPeersRequest{
				TaskId:    "task-999",
				WorkerId:  "worker-1",
				AuthToken: "test-token",
			},
			wantSuccess:   true,
			wantPeerCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			resp, err := handler.GetTaskPeers(ctx, tt.req)
			if err != nil {
				t.Fatalf("GetTaskPeers() error = %v", err)
			}

			if resp.Success != tt.wantSuccess {
				t.Errorf("GetTaskPeers() success = %v, want %v", resp.Success, tt.wantSuccess)
			}

			if !tt.wantSuccess && resp.StatusMessage != tt.wantErrMessage {
				t.Errorf("GetTaskPeers() status_message = %v, want %v", resp.StatusMessage, tt.wantErrMessage)
			}

			if tt.wantSuccess && len(resp.Peers) != tt.wantPeerCount {
				t.Errorf("GetTaskPeers() peer count = %v, want %v", len(resp.Peers), tt.wantPeerCount)
			}

			// 驗證 Peer 資訊
			if tt.wantSuccess && len(resp.Peers) > 0 {
				for _, peer := range resp.Peers {
					if peer.WorkerId == "" {
						t.Error("Peer worker_id is empty")
					}
					if peer.VirtualIp == "" {
						t.Error("Peer virtual_ip is empty")
					}
					if peer.Hostname == "" {
						t.Error("Peer hostname is empty")
					}
				}
			}
		})
	}
}

func TestLeaveVPN(t *testing.T) {
	handler := setupTestVPNHandler(t)
	ctx := context.Background()

	// 註冊一個 Worker
	_, err := handler.JoinVPN(ctx, &pb.JoinVPNRequest{
		WorkerId:  "worker-1",
		Hostname:  "worker1.local",
		AuthToken: "test-token",
	})
	if err != nil {
		t.Fatalf("Failed to join worker-1: %v", err)
	}

	tests := []struct {
		name           string
		req            *pb.LeaveVPNRequest
		wantSuccess    bool
		wantErrMessage string
	}{
		{
			name: "valid request",
			req: &pb.LeaveVPNRequest{
				WorkerId:  "worker-1",
				AuthToken: "test-token",
			},
			wantSuccess: true,
		},
		{
			name: "missing worker_id",
			req: &pb.LeaveVPNRequest{
				AuthToken: "test-token",
			},
			wantSuccess:    false,
			wantErrMessage: "worker_id is required",
		},
		{
			name: "non-existing worker",
			req: &pb.LeaveVPNRequest{
				WorkerId:  "worker-999",
				AuthToken: "test-token",
			},
			wantSuccess: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			resp, err := handler.LeaveVPN(ctx, tt.req)
			if err != nil {
				t.Fatalf("LeaveVPN() error = %v", err)
			}

			if resp.Success != tt.wantSuccess {
				t.Errorf("LeaveVPN() success = %v, want %v", resp.Success, tt.wantSuccess)
			}

			if !tt.wantSuccess && tt.wantErrMessage != "" && resp.StatusMessage != tt.wantErrMessage {
				t.Errorf("LeaveVPN() status_message = %v, want %v", resp.StatusMessage, tt.wantErrMessage)
			}
		})
	}
}

func TestUpdateVPNStatus(t *testing.T) {
	handler := setupTestVPNHandler(t)
	ctx := context.Background()

	// 註冊一個 Worker
	_, err := handler.JoinVPN(ctx, &pb.JoinVPNRequest{
		WorkerId:  "worker-1",
		Hostname:  "worker1.local",
		AuthToken: "test-token",
	})
	if err != nil {
		t.Fatalf("Failed to join worker-1: %v", err)
	}

	tests := []struct {
		name           string
		req            *pb.UpdateVPNStatusRequest
		wantSuccess    bool
		wantErrMessage string
	}{
		{
			name: "update to offline",
			req: &pb.UpdateVPNStatusRequest{
				WorkerId:  "worker-1",
				VirtualIp: "100.64.0.1",
				Online:    false,
				AuthToken: "test-token",
			},
			wantSuccess: true,
		},
		{
			name: "update to online",
			req: &pb.UpdateVPNStatusRequest{
				WorkerId:  "worker-1",
				VirtualIp: "100.64.0.1",
				Online:    true,
				AuthToken: "test-token",
			},
			wantSuccess: true,
		},
		{
			name: "missing worker_id",
			req: &pb.UpdateVPNStatusRequest{
				VirtualIp: "100.64.0.1",
				Online:    true,
				AuthToken: "test-token",
			},
			wantSuccess:    false,
			wantErrMessage: "worker_id is required",
		},
		{
			name: "non-existing worker",
			req: &pb.UpdateVPNStatusRequest{
				WorkerId:  "worker-999",
				VirtualIp: "100.64.0.99",
				Online:    true,
				AuthToken: "test-token",
			},
			wantSuccess: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			resp, err := handler.UpdateVPNStatus(ctx, tt.req)
			if err != nil {
				t.Fatalf("UpdateVPNStatus() error = %v", err)
			}

			if resp.Success != tt.wantSuccess {
				t.Errorf("UpdateVPNStatus() success = %v, want %v", resp.Success, tt.wantSuccess)
			}

			if !tt.wantSuccess && tt.wantErrMessage != "" && resp.StatusMessage != tt.wantErrMessage {
				t.Errorf("UpdateVPNStatus() status_message = %v, want %v", resp.StatusMessage, tt.wantErrMessage)
			}
		})
	}
}

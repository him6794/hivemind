package handler

import (
	"context"
	"fmt"

	"github.com/rs/zerolog/log"
	"hivemind/services/nodepool/internal/vpn"
	"hivemind/services/nodepool/pb"
)

// VPNHandler 實作 VPNService gRPC 服務
type VPNHandler struct {
	pb.UnimplementedVPNServiceServer
	vpnManager *vpn.HeadscaleManager
}

// NewVPNHandler 創建新的 VPN 處理器
func NewVPNHandler(vpnManager *vpn.HeadscaleManager) *VPNHandler {
	return &VPNHandler{
		vpnManager: vpnManager,
	}
}

// JoinVPN 處理 Worker 加入 VPN 網路的請求
func (h *VPNHandler) JoinVPN(ctx context.Context, req *pb.JoinVPNRequest) (*pb.JoinVPNResponse, error) {
	log.Info().
		Str("worker_id", req.WorkerId).
		Str("hostname", req.Hostname).
		Msg("Received JoinVPN request")

	// 驗證請求參數
	if req.WorkerId == "" {
		return &pb.JoinVPNResponse{
			Success:       false,
			StatusMessage: "worker_id is required",
		}, nil
	}

	if req.Hostname == "" {
		return &pb.JoinVPNResponse{
			Success:       false,
			StatusMessage: "hostname is required",
		}, nil
	}

	// TODO: 驗證 auth_token
	if req.AuthToken == "" {
		log.Warn().
			Str("worker_id", req.WorkerId).
			Msg("No auth token provided")
	}

	// 註冊 Worker 到 VPN 網路
	worker, err := h.vpnManager.RegisterWorker(ctx, req.WorkerId, req.Hostname)
	if err != nil {
		log.Error().
			Err(err).
			Str("worker_id", req.WorkerId).
			Msg("Failed to register worker")
		return &pb.JoinVPNResponse{
			Success:       false,
			StatusMessage: fmt.Sprintf("failed to register worker: %v", err),
		}, nil
	}

	// 取得 DERP 配置
	derpMap, err := h.vpnManager.GetDERPMap()
	if err != nil {
		log.Warn().
			Err(err).
			Str("worker_id", req.WorkerId).
			Msg("Failed to get DERP map, using empty map")
		derpMap = "{}"
	}

	log.Info().
		Str("worker_id", req.WorkerId).
		Str("virtual_ip", worker.VirtualIP).
		Msg("Worker joined VPN successfully")

	return &pb.JoinVPNResponse{
		Success:       true,
		StatusMessage: "joined VPN successfully",
		VirtualIp:     worker.VirtualIP,
		AuthKey:       worker.AuthKey,
		DerpMap:       derpMap,
	}, nil
}

// GetTaskPeers 處理取得任務相關 Peer 節點的請求
func (h *VPNHandler) GetTaskPeers(ctx context.Context, req *pb.GetTaskPeersRequest) (*pb.GetTaskPeersResponse, error) {
	log.Info().
		Str("task_id", req.TaskId).
		Str("worker_id", req.WorkerId).
		Msg("Received GetTaskPeers request")

	// 驗證請求參數
	if req.TaskId == "" {
		return &pb.GetTaskPeersResponse{
			Success:       false,
			StatusMessage: "task_id is required",
		}, nil
	}

	if req.WorkerId == "" {
		return &pb.GetTaskPeersResponse{
			Success:       false,
			StatusMessage: "worker_id is required",
		}, nil
	}

	// TODO: 驗證 auth_token
	if req.AuthToken == "" {
		log.Warn().
			Str("worker_id", req.WorkerId).
			Msg("No auth token provided")
	}

	// 取得任務相關的 Peer 節點
	peers, err := h.vpnManager.GetTaskPeers(ctx, req.TaskId, req.WorkerId)
	if err != nil {
		log.Error().
			Err(err).
			Str("task_id", req.TaskId).
			Str("worker_id", req.WorkerId).
			Msg("Failed to get task peers")
		return &pb.GetTaskPeersResponse{
			Success:       false,
			StatusMessage: fmt.Sprintf("failed to get task peers: %v", err),
		}, nil
	}

	// 轉換為 protobuf 格式
	pbPeers := make([]*pb.PeerInfo, 0, len(peers))
	for _, peer := range peers {
		pbPeers = append(pbPeers, &pb.PeerInfo{
			WorkerId:  peer.WorkerID,
			VirtualIp: peer.VirtualIP,
			Hostname:  peer.Hostname,
			Online:    peer.Online,
			LastSeen:  peer.LastSeen,
		})
	}

	log.Info().
		Str("task_id", req.TaskId).
		Str("worker_id", req.WorkerId).
		Int("peer_count", len(pbPeers)).
		Msg("Retrieved task peers successfully")

	return &pb.GetTaskPeersResponse{
		Success:       true,
		StatusMessage: "retrieved task peers successfully",
		Peers:         pbPeers,
	}, nil
}

// LeaveVPN 處理 Worker 離開 VPN 網路的請求
func (h *VPNHandler) LeaveVPN(ctx context.Context, req *pb.LeaveVPNRequest) (*pb.LeaveVPNResponse, error) {
	log.Info().
		Str("worker_id", req.WorkerId).
		Msg("Received LeaveVPN request")

	// 驗證請求參數
	if req.WorkerId == "" {
		return &pb.LeaveVPNResponse{
			Success:       false,
			StatusMessage: "worker_id is required",
		}, nil
	}

	// TODO: 驗證 auth_token
	if req.AuthToken == "" {
		log.Warn().
			Str("worker_id", req.WorkerId).
			Msg("No auth token provided")
	}

	// 註銷 Worker
	err := h.vpnManager.UnregisterWorker(ctx, req.WorkerId)
	if err != nil {
		log.Error().
			Err(err).
			Str("worker_id", req.WorkerId).
			Msg("Failed to unregister worker")
		return &pb.LeaveVPNResponse{
			Success:       false,
			StatusMessage: fmt.Sprintf("failed to unregister worker: %v", err),
		}, nil
	}

	log.Info().
		Str("worker_id", req.WorkerId).
		Msg("Worker left VPN successfully")

	return &pb.LeaveVPNResponse{
		Success:       true,
		StatusMessage: "left VPN successfully",
	}, nil
}

// UpdateVPNStatus 處理更新 Worker VPN 狀態的請求（心跳）
func (h *VPNHandler) UpdateVPNStatus(ctx context.Context, req *pb.UpdateVPNStatusRequest) (*pb.UpdateVPNStatusResponse, error) {
	log.Debug().
		Str("worker_id", req.WorkerId).
		Bool("online", req.Online).
		Msg("Received UpdateVPNStatus request")

	// 驗證請求參數
	if req.WorkerId == "" {
		return &pb.UpdateVPNStatusResponse{
			Success:       false,
			StatusMessage: "worker_id is required",
		}, nil
	}

	// TODO: 驗證 auth_token
	if req.AuthToken == "" {
		log.Warn().
			Str("worker_id", req.WorkerId).
			Msg("No auth token provided")
	}

	// 更新 Worker 狀態
	err := h.vpnManager.UpdateWorkerStatus(ctx, req.WorkerId, req.Online)
	if err != nil {
		log.Error().
			Err(err).
			Str("worker_id", req.WorkerId).
			Msg("Failed to update worker status")
		return &pb.UpdateVPNStatusResponse{
			Success:       false,
			StatusMessage: fmt.Sprintf("failed to update worker status: %v", err),
		}, nil
	}

	return &pb.UpdateVPNStatusResponse{
		Success:       true,
		StatusMessage: "status updated successfully",
	}, nil
}

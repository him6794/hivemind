package server

import (
	"context"
	"fmt"
	"net"
	"os"
	"os/signal"
	"syscall"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
	"google.golang.org/grpc"
	"hivemind/services/nodepool/internal/handler"
	"hivemind/services/nodepool/internal/vpn"
	"hivemind/services/nodepool/pb"
	"hivemind/services/nodepool/pkg/config"
)

// Start starts the nodepool gRPC server. Handlers live in pkg/handlers.
func Start() error {
	// 初始化日誌
	log.Logger = log.Output(zerolog.ConsoleWriter{Out: os.Stderr})

	log.Info().Msg("Starting Nodepool server")

	// 載入 VPN 配置
	vpnConfig := config.LoadVPNConfig()

	// 初始化 VPN 管理器（如果啟用）
	var vpnManager *vpn.HeadscaleManager
	var err error

	if vpnConfig.Enabled {
		log.Info().Msg("VPN feature is enabled, initializing Headscale manager")

		vpnManagerConfig := &vpn.Config{
			ServerURL:           vpnConfig.ServerURL,
			ListenAddr:          vpnConfig.ListenAddr,
			GRPCListenAddr:      vpnConfig.GRPCListenAddr,
			IPPrefix:            vpnConfig.IPPrefix,
			BaseDomain:          vpnConfig.BaseDomain,
			DERPMapURL:          vpnConfig.DERPMapURL,
			DERPAutoUpdate:      vpnConfig.DERPAutoUpdate,
			EphemeralNodes:      vpnConfig.EphemeralNodes,
			NodeExpiry:          vpnConfig.NodeExpiry,
			DBType:              vpnConfig.DBType,
			DBPath:              vpnConfig.DBPath,
			DBHost:              vpnConfig.DBHost,
			DBPort:              vpnConfig.DBPort,
			DBName:              vpnConfig.DBName,
			DBUser:              vpnConfig.DBUser,
			DBPassword:          vpnConfig.DBPassword,
			PrivateKeyPath:      vpnConfig.PrivateKeyPath,
			NoisePrivateKeyPath: vpnConfig.NoisePrivateKeyPath,
		}

		vpnManager, err = vpn.NewHeadscaleManager(vpnManagerConfig)
		if err != nil {
			return fmt.Errorf("failed to create VPN manager: %w", err)
		}

		// 啟動 VPN 管理器
		ctx := context.Background()
		if err := vpnManager.Start(ctx); err != nil {
			return fmt.Errorf("failed to start VPN manager: %w", err)
		}

		log.Info().Msg("VPN manager started successfully")
	} else {
		log.Info().Msg("VPN feature is disabled")
	}

	// 啟動 gRPC 伺服器
	grpcPort := config.GRPCPort()
	lis, err := net.Listen("tcp", grpcPort)
	if err != nil {
		return fmt.Errorf("listen: %w", err)
	}

	s := grpc.NewServer()

	// 註冊 VPN 服務（如果啟用）
	if vpnConfig.Enabled && vpnManager != nil {
		vpnHandler := handler.NewVPNHandler(vpnManager)
		pb.RegisterVPNServiceServer(s, vpnHandler)
		log.Info().Msg("VPN service registered")
	}

	// TODO: 註冊其他服務
	// pb.RegisterNodeManagerServiceServer(s, handlers.NewNodeManager())
	// pb.RegisterUserServiceServer(s, handlers.NewUserService())
	// pb.RegisterMasterNodeServiceServer(s, handlers.NewMasterNodeService())
	// pb.RegisterWorkerNodeServiceServer(s, handlers.NewWorkerNodeService())

	// 在 goroutine 中啟動伺服器
	errChan := make(chan error, 1)
	go func() {
		log.Info().Str("port", grpcPort).Msg("gRPC server listening")
		if err := s.Serve(lis); err != nil {
			errChan <- fmt.Errorf("gRPC serve error: %w", err)
		}
	}()

	// 等待中斷信號
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	select {
	case err := <-errChan:
		log.Error().Err(err).Msg("Server error")
		return err
	case sig := <-sigChan:
		log.Info().Str("signal", sig.String()).Msg("Received shutdown signal")
	}

	// 優雅關閉
	log.Info().Msg("Shutting down server")
	s.GracefulStop()

	// 停止 VPN 管理器
	if vpnConfig.Enabled && vpnManager != nil {
		if err := vpnManager.Stop(); err != nil {
			log.Error().Err(err).Msg("Failed to stop VPN manager")
		}
	}

	log.Info().Msg("Server stopped")
	return nil
}

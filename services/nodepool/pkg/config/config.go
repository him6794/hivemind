package config

// config.go: configuration loader for nodepool service

import (
	"os"
	"strconv"
	"time"
)

func GetEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func GRPCPort() string {
	return GetEnv("NODEPOOL_GRPC_PORT", ":50051")
}

// VPNConfig 包含 VPN/Headscale 相關配置
type VPNConfig struct {
	// Headscale 伺服器配置
	ServerURL      string        // Nodepool 的公開 URL
	ListenAddr     string        // Headscale 監聽地址
	GRPCListenAddr string        // Headscale gRPC 監聽地址

	// 網路配置
	IPPrefix       string        // IP 前綴，預設 "100.64.0.0/10"
	BaseDomain     string        // 基礎域名，例如 "hivemind.local"

	// DERP 配置
	DERPMapURL     string        // DERP 伺服器配置 URL
	DERPAutoUpdate bool          // 是否自動更新 DERP 配置

	// 節點管理
	EphemeralNodes bool          // Worker 離線後自動清理
	NodeExpiry     time.Duration // 節點過期時間

	// 資料庫配置
	DBType         string        // 資料庫類型: sqlite, postgres
	DBPath         string        // SQLite 資料庫路徑
	DBHost         string        // PostgreSQL 主機
	DBPort         int           // PostgreSQL 埠
	DBName         string        // PostgreSQL 資料庫名稱
	DBUser         string        // PostgreSQL 使用者
	DBPassword     string        // PostgreSQL 密碼

	// 安全配置
	PrivateKeyPath string        // 私鑰檔案路徑
	NoisePrivateKeyPath string   // Noise 協議私鑰路徑

	// 功能開關
	Enabled        bool          // 是否啟用 VPN 功能
}

// LoadVPNConfig 從環境變數載入 VPN 配置
func LoadVPNConfig() *VPNConfig {
	enabled := GetEnv("VPN_ENABLED", "true") == "true"

	nodeExpiryStr := GetEnv("VPN_NODE_EXPIRY", "24h")
	nodeExpiry, err := time.ParseDuration(nodeExpiryStr)
	if err != nil {
		nodeExpiry = 24 * time.Hour
	}

	dbPort, _ := strconv.Atoi(GetEnv("VPN_DB_PORT", "5432"))

	return &VPNConfig{
		Enabled:             enabled,
		ServerURL:           GetEnv("VPN_SERVER_URL", "http://localhost:8080"),
		ListenAddr:          GetEnv("VPN_LISTEN_ADDR", "0.0.0.0:8080"),
		GRPCListenAddr:      GetEnv("VPN_GRPC_LISTEN_ADDR", "0.0.0.0:50443"),
		IPPrefix:            GetEnv("VPN_IP_PREFIX", "100.64.0.0/10"),
		BaseDomain:          GetEnv("VPN_BASE_DOMAIN", "hivemind.local"),
		DERPMapURL:          GetEnv("VPN_DERP_MAP_URL", ""),
		DERPAutoUpdate:      GetEnv("VPN_DERP_AUTO_UPDATE", "true") == "true",
		EphemeralNodes:      GetEnv("VPN_EPHEMERAL_NODES", "true") == "true",
		NodeExpiry:          nodeExpiry,
		DBType:              GetEnv("VPN_DB_TYPE", "sqlite"),
		DBPath:              GetEnv("VPN_DB_PATH", "/var/lib/headscale/db.sqlite"),
		DBHost:              GetEnv("VPN_DB_HOST", "localhost"),
		DBPort:              dbPort,
		DBName:              GetEnv("VPN_DB_NAME", "headscale"),
		DBUser:              GetEnv("VPN_DB_USER", "headscale"),
		DBPassword:          GetEnv("VPN_DB_PASSWORD", ""),
		PrivateKeyPath:      GetEnv("VPN_PRIVATE_KEY_PATH", "/var/lib/headscale/private.key"),
		NoisePrivateKeyPath: GetEnv("VPN_NOISE_PRIVATE_KEY_PATH", "/var/lib/headscale/noise_private.key"),
	}
}

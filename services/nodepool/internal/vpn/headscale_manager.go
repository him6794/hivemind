package vpn

import (
	"context"
	"fmt"
	"net/netip"
	"sync"
	"time"

	"github.com/rs/zerolog/log"
	"gorm.io/gorm"
)

// HeadscaleManager 管理嵌入式 Headscale 伺服器
type HeadscaleManager struct {
	config *Config
	db     *gorm.DB
	mu     sync.RWMutex

	// Worker 節點註冊表
	workers map[string]*WorkerNode

	// 任務到 Worker 的映射
	taskWorkers map[string][]string
}

// Config Headscale 管理器配置
type Config struct {
	ServerURL          string
	ListenAddr         string
	GRPCListenAddr     string
	IPPrefix           string
	BaseDomain         string
	DERPMapURL         string
	DERPAutoUpdate     bool
	EphemeralNodes     bool
	NodeExpiry         time.Duration
	DBType             string
	DBPath             string
	DBHost             string
	DBPort             int
	DBName             string
	DBUser             string
	DBPassword         string
	PrivateKeyPath     string
	NoisePrivateKeyPath string
}

// WorkerNode 代表一個 Worker 節點
type WorkerNode struct {
	WorkerID    string
	Hostname    string
	VirtualIP   string
	AuthKey     string
	Online      bool
	LastSeen    time.Time
	RegisteredAt time.Time
}

// PeerInfo Peer 節點資訊
type PeerInfo struct {
	WorkerID  string
	VirtualIP string
	Hostname  string
	Online    bool
	LastSeen  int64
}

// NewHeadscaleManager 創建新的 Headscale 管理器
func NewHeadscaleManager(cfg *Config) (*HeadscaleManager, error) {
	if cfg == nil {
		return nil, fmt.Errorf("config cannot be nil")
	}

	// 驗證配置
	if err := validateConfig(cfg); err != nil {
		return nil, fmt.Errorf("invalid config: %w", err)
	}

	// 解析 IP 前綴
	_, err := netip.ParsePrefix(cfg.IPPrefix)
	if err != nil {
		return nil, fmt.Errorf("invalid IP prefix %s: %w", cfg.IPPrefix, err)
	}

	manager := &HeadscaleManager{
		config:      cfg,
		workers:     make(map[string]*WorkerNode),
		taskWorkers: make(map[string][]string),
	}

	// 初始化資料庫連接
	if err := manager.initDatabase(); err != nil {
		return nil, fmt.Errorf("failed to initialize database: %w", err)
	}

	log.Info().
		Str("server_url", cfg.ServerURL).
		Str("ip_prefix", cfg.IPPrefix).
		Msg("Headscale manager initialized")

	return manager, nil
}

// validateConfig 驗證配置
func validateConfig(cfg *Config) error {
	if cfg.ServerURL == "" {
		return fmt.Errorf("ServerURL is required")
	}
	if cfg.IPPrefix == "" {
		return fmt.Errorf("IPPrefix is required")
	}
	if cfg.BaseDomain == "" {
		return fmt.Errorf("BaseDomain is required")
	}
	return nil
}

// initDatabase 初始化資料庫連接
func (m *HeadscaleManager) initDatabase() error {
	// TODO: 實際實作時需要初始化 GORM 連接
	// 這裡先預留介面
	log.Info().
		Str("db_type", m.config.DBType).
		Msg("Database initialization placeholder")
	return nil
}

// Start 啟動 Headscale 伺服器
func (m *HeadscaleManager) Start(ctx context.Context) error {
	log.Info().Msg("Starting Headscale manager")

	// TODO: 實際啟動 Headscale 伺服器
	// 這裡需要整合 github.com/juanfont/headscale/hscontrol

	// 啟動清理過期節點的 goroutine
	go m.cleanupExpiredNodes(ctx)

	return nil
}

// Stop 停止 Headscale 伺服器
func (m *HeadscaleManager) Stop() error {
	log.Info().Msg("Stopping Headscale manager")
	// TODO: 優雅關閉 Headscale 伺服器
	return nil
}

// RegisterWorker 註冊 Worker 節點到 VPN 網路
func (m *HeadscaleManager) RegisterWorker(ctx context.Context, workerID, hostname string) (*WorkerNode, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	// 檢查是否已註冊
	if worker, exists := m.workers[workerID]; exists {
		log.Info().
			Str("worker_id", workerID).
			Str("virtual_ip", worker.VirtualIP).
			Msg("Worker already registered, returning existing registration")
		worker.Online = true
		worker.LastSeen = time.Now()
		return worker, nil
	}

	// 分配虛擬 IP
	virtualIP, err := m.allocateIP()
	if err != nil {
		return nil, fmt.Errorf("failed to allocate IP: %w", err)
	}

	// 生成認證金鑰
	authKey, err := m.generateAuthKey(workerID)
	if err != nil {
		return nil, fmt.Errorf("failed to generate auth key: %w", err)
	}

	// 創建 Worker 節點記錄
	worker := &WorkerNode{
		WorkerID:     workerID,
		Hostname:     hostname,
		VirtualIP:    virtualIP,
		AuthKey:      authKey,
		Online:       true,
		LastSeen:     time.Now(),
		RegisteredAt: time.Now(),
	}

	m.workers[workerID] = worker

	log.Info().
		Str("worker_id", workerID).
		Str("hostname", hostname).
		Str("virtual_ip", virtualIP).
		Msg("Worker registered successfully")

	return worker, nil
}

// UnregisterWorker 從 VPN 網路註銷 Worker 節點
func (m *HeadscaleManager) UnregisterWorker(ctx context.Context, workerID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	worker, exists := m.workers[workerID]
	if !exists {
		return fmt.Errorf("worker %s not found", workerID)
	}

	// 從所有任務中移除該 Worker
	for taskID, workers := range m.taskWorkers {
		newWorkers := make([]string, 0)
		for _, wid := range workers {
			if wid != workerID {
				newWorkers = append(newWorkers, wid)
			}
		}
		m.taskWorkers[taskID] = newWorkers
	}

	delete(m.workers, workerID)

	log.Info().
		Str("worker_id", workerID).
		Str("virtual_ip", worker.VirtualIP).
		Msg("Worker unregistered successfully")

	return nil
}

// UpdateWorkerStatus 更新 Worker 狀態（心跳）
func (m *HeadscaleManager) UpdateWorkerStatus(ctx context.Context, workerID string, online bool) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	worker, exists := m.workers[workerID]
	if !exists {
		return fmt.Errorf("worker %s not found", workerID)
	}

	worker.Online = online
	worker.LastSeen = time.Now()

	log.Debug().
		Str("worker_id", workerID).
		Bool("online", online).
		Msg("Worker status updated")

	return nil
}

// GetTaskPeers 取得任務相關的 Peer 節點列表
func (m *HeadscaleManager) GetTaskPeers(ctx context.Context, taskID, workerID string) ([]*PeerInfo, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	// 取得任務相關的 Worker 列表
	workerIDs, exists := m.taskWorkers[taskID]
	if !exists {
		return []*PeerInfo{}, nil
	}

	peers := make([]*PeerInfo, 0, len(workerIDs))
	for _, wid := range workerIDs {
		// 排除請求者自己
		if wid == workerID {
			continue
		}

		worker, exists := m.workers[wid]
		if !exists {
			continue
		}

		peers = append(peers, &PeerInfo{
			WorkerID:  worker.WorkerID,
			VirtualIP: worker.VirtualIP,
			Hostname:  worker.Hostname,
			Online:    worker.Online,
			LastSeen:  worker.LastSeen.Unix(),
		})
	}

	log.Debug().
		Str("task_id", taskID).
		Str("worker_id", workerID).
		Int("peer_count", len(peers)).
		Msg("Retrieved task peers")

	return peers, nil
}

// AssignWorkerToTask 將 Worker 分配到任務
func (m *HeadscaleManager) AssignWorkerToTask(ctx context.Context, taskID, workerID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	// 檢查 Worker 是否存在
	if _, exists := m.workers[workerID]; !exists {
		return fmt.Errorf("worker %s not found", workerID)
	}

	// 將 Worker 加入任務列表
	workers := m.taskWorkers[taskID]
	for _, wid := range workers {
		if wid == workerID {
			// 已經在列表中
			return nil
		}
	}

	m.taskWorkers[taskID] = append(workers, workerID)

	log.Info().
		Str("task_id", taskID).
		Str("worker_id", workerID).
		Msg("Worker assigned to task")

	return nil
}

// RemoveWorkerFromTask 從任務中移除 Worker
func (m *HeadscaleManager) RemoveWorkerFromTask(ctx context.Context, taskID, workerID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	workers, exists := m.taskWorkers[taskID]
	if !exists {
		return nil
	}

	newWorkers := make([]string, 0)
	for _, wid := range workers {
		if wid != workerID {
			newWorkers = append(newWorkers, wid)
		}
	}

	if len(newWorkers) == 0 {
		delete(m.taskWorkers, taskID)
	} else {
		m.taskWorkers[taskID] = newWorkers
	}

	log.Info().
		Str("task_id", taskID).
		Str("worker_id", workerID).
		Msg("Worker removed from task")

	return nil
}

// GetDERPMap 取得 DERP 伺服器配置
func (m *HeadscaleManager) GetDERPMap() (string, error) {
	// TODO: 實作 DERP 配置生成或從 URL 取得
	// 暫時返回預設配置
	defaultDERPMap := `{
		"Regions": {
			"900": {
				"RegionID": 900,
				"RegionCode": "default",
				"RegionName": "Default DERP",
				"Nodes": [
					{
						"Name": "default",
						"RegionID": 900,
						"HostName": "derp.hivemind.local",
						"DERPPort": 3478
					}
				]
			}
		}
	}`

	return defaultDERPMap, nil
}

// allocateIP 分配虛擬 IP 地址
func (m *HeadscaleManager) allocateIP() (string, error) {
	// 簡單的 IP 分配策略：基於已註冊節點數量
	// 實際實作應該使用更複雜的 IP 池管理
	ipNum := len(m.workers) + 1

	// 100.64.0.0/10 範圍
	ip := fmt.Sprintf("100.64.%d.%d", (ipNum/256)%64, ipNum%256)

	// 檢查 IP 是否已被使用
	for _, worker := range m.workers {
		if worker.VirtualIP == ip {
			// IP 衝突，遞增重試
			ipNum++
			ip = fmt.Sprintf("100.64.%d.%d", (ipNum/256)%64, ipNum%256)
		}
	}

	return ip, nil
}

// generateAuthKey 生成認證金鑰
func (m *HeadscaleManager) generateAuthKey(workerID string) (string, error) {
	// TODO: 實作實際的 Headscale PreAuthKey 生成
	// 暫時返回簡單的金鑰
	authKey := fmt.Sprintf("authkey-%s-%d", workerID, time.Now().Unix())
	return authKey, nil
}

// cleanupExpiredNodes 清理過期節點
func (m *HeadscaleManager) cleanupExpiredNodes(ctx context.Context) {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			m.performCleanup()
		}
	}
}

// performCleanup 執行清理操作
func (m *HeadscaleManager) performCleanup() {
	if !m.config.EphemeralNodes {
		return
	}

	m.mu.Lock()
	defer m.mu.Unlock()

	now := time.Now()
	expiredWorkers := make([]string, 0)

	for workerID, worker := range m.workers {
		if !worker.Online && now.Sub(worker.LastSeen) > m.config.NodeExpiry {
			expiredWorkers = append(expiredWorkers, workerID)
		}
	}

	for _, workerID := range expiredWorkers {
		delete(m.workers, workerID)
		log.Info().
			Str("worker_id", workerID).
			Msg("Expired worker cleaned up")
	}

	if len(expiredWorkers) > 0 {
		log.Info().
			Int("count", len(expiredWorkers)).
			Msg("Cleanup completed")
	}
}

// GetWorkerCount 取得已註冊的 Worker 數量
func (m *HeadscaleManager) GetWorkerCount() int {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return len(m.workers)
}

// GetOnlineWorkerCount 取得在線的 Worker 數量
func (m *HeadscaleManager) GetOnlineWorkerCount() int {
	m.mu.RLock()
	defer m.mu.RUnlock()

	count := 0
	for _, worker := range m.workers {
		if worker.Online {
			count++
		}
	}
	return count
}

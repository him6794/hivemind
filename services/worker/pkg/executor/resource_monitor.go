package executor

import (
	"context"
	"fmt"
	"sync"
	"time"

	"github.com/shirou/gopsutil/v3/process"
)

// ResourceUsage 資源使用情況
type ResourceUsage struct {
	CPUPercent    float64   // CPU 使用率 (%)
	MemoryMB      int64     // 記憶體使用 (MB)
	MemoryPercent float32   // 記憶體使用率 (%)
	Timestamp     time.Time // 時間戳
}

// ResourceMonitor 資源監控器
type ResourceMonitor struct {
	pid      int32
	interval time.Duration
	callback func(ResourceUsage)

	ctx    context.Context
	cancel context.CancelFunc
	wg     sync.WaitGroup

	mu           sync.RWMutex
	currentUsage ResourceUsage
	history      []ResourceUsage
	maxHistory   int
}

// NewResourceMonitor 創建資源監控器
func NewResourceMonitor(pid int, interval time.Duration, callback func(ResourceUsage)) *ResourceMonitor {
	ctx, cancel := context.WithCancel(context.Background())

	return &ResourceMonitor{
		pid:        int32(pid),
		interval:   interval,
		callback:   callback,
		ctx:        ctx,
		cancel:     cancel,
		maxHistory: 100, // 保留最近 100 個數據點
		history:    make([]ResourceUsage, 0, 100),
	}
}

// Start 啟動監控
func (m *ResourceMonitor) Start() error {
	// 檢查進程是否存在
	proc, err := process.NewProcess(m.pid)
	if err != nil {
		return fmt.Errorf("process not found: %w", err)
	}

	m.wg.Add(1)
	go m.monitorLoop(proc)

	return nil
}

// Stop 停止監控
func (m *ResourceMonitor) Stop() {
	m.cancel()
	m.wg.Wait()
}

// GetCurrentUsage 獲取當前資源使用情況
func (m *ResourceMonitor) GetCurrentUsage() ResourceUsage {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.currentUsage
}

// GetHistory 獲取歷史資源使用記錄
func (m *ResourceMonitor) GetHistory() []ResourceUsage {
	m.mu.RLock()
	defer m.mu.RUnlock()

	// 返回副本
	history := make([]ResourceUsage, len(m.history))
	copy(history, m.history)
	return history
}

// monitorLoop 監控循環
func (m *ResourceMonitor) monitorLoop(proc *process.Process) {
	defer m.wg.Done()

	ticker := time.NewTicker(m.interval)
	defer ticker.Stop()

	for {
		select {
		case <-m.ctx.Done():
			return

		case <-ticker.C:
			usage, err := m.collectUsage(proc)
			if err != nil {
				// 進程可能已結束
				if !m.isProcessRunning(proc) {
					return
				}
				// 其他錯誤，繼續監控
				continue
			}

			// 更新當前使用情況
			m.mu.Lock()
			m.currentUsage = usage
			m.history = append(m.history, usage)

			// 限制歷史記錄大小
			if len(m.history) > m.maxHistory {
				m.history = m.history[1:]
			}
			m.mu.Unlock()

			// 回調
			if m.callback != nil {
				m.callback(usage)
			}
		}
	}
}

// collectUsage 收集資源使用情況
func (m *ResourceMonitor) collectUsage(proc *process.Process) (ResourceUsage, error) {
	usage := ResourceUsage{
		Timestamp: time.Now(),
	}

	// CPU 使用率
	cpuPercent, err := proc.CPUPercent()
	if err != nil {
		return usage, fmt.Errorf("get cpu percent: %w", err)
	}
	usage.CPUPercent = cpuPercent

	// 記憶體使用
	memInfo, err := proc.MemoryInfo()
	if err != nil {
		return usage, fmt.Errorf("get memory info: %w", err)
	}
	usage.MemoryMB = int64(memInfo.RSS / 1024 / 1024) // 轉換為 MB

	// 記憶體使用率
	memPercent, err := proc.MemoryPercent()
	if err != nil {
		return usage, fmt.Errorf("get memory percent: %w", err)
	}
	usage.MemoryPercent = memPercent

	return usage, nil
}

// isProcessRunning 檢查進程是否仍在運行
func (m *ResourceMonitor) isProcessRunning(proc *process.Process) bool {
	running, err := proc.IsRunning()
	if err != nil {
		return false
	}
	return running
}

// CheckLimits 檢查是否超過資源限制
func (m *ResourceMonitor) CheckLimits(limits ResourceLimits) error {
	usage := m.GetCurrentUsage()

	// 檢查記憶體限制
	if limits.MemoryMB > 0 && usage.MemoryMB > int64(limits.MemoryMB) {
		return fmt.Errorf("memory limit exceeded: %d MB > %d MB",
			usage.MemoryMB, limits.MemoryMB)
	}

	// 檢查 CPU 使用率（可選，通常不強制限制）
	// if usage.CPUPercent > 100.0 {
	// 	return fmt.Errorf("CPU usage too high: %.2f%%", usage.CPUPercent)
	// }

	return nil
}

// GetAverageUsage 計算平均資源使用
func (m *ResourceMonitor) GetAverageUsage() ResourceUsage {
	m.mu.RLock()
	defer m.mu.RUnlock()

	if len(m.history) == 0 {
		return ResourceUsage{}
	}

	var totalCPU float64
	var totalMem int64
	var totalMemPercent float32

	for _, usage := range m.history {
		totalCPU += usage.CPUPercent
		totalMem += usage.MemoryMB
		totalMemPercent += usage.MemoryPercent
	}

	count := float64(len(m.history))
	return ResourceUsage{
		CPUPercent:    totalCPU / count,
		MemoryMB:      int64(float64(totalMem) / count),
		MemoryPercent: totalMemPercent / float32(count),
		Timestamp:     time.Now(),
	}
}

// GetPeakUsage 獲取峰值資源使用
func (m *ResourceMonitor) GetPeakUsage() ResourceUsage {
	m.mu.RLock()
	defer m.mu.RUnlock()

	if len(m.history) == 0 {
		return ResourceUsage{}
	}

	peak := m.history[0]
	for _, usage := range m.history[1:] {
		if usage.CPUPercent > peak.CPUPercent {
			peak.CPUPercent = usage.CPUPercent
		}
		if usage.MemoryMB > peak.MemoryMB {
			peak.MemoryMB = usage.MemoryMB
			peak.MemoryPercent = usage.MemoryPercent
		}
	}

	return peak
}

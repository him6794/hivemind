package executor

import (
	"fmt"
	"os"
	"syscall"
	"time"
)

// TerminationReason 終止原因
type TerminationReason string

const (
	TerminationMemoryLimit TerminationReason = "memory_limit_exceeded"
	TerminationTimeout     TerminationReason = "execution_timeout"
	TerminationUserRequest TerminationReason = "user_requested"
	TerminationError       TerminationReason = "execution_error"
)

// TerminationInfo 終止資訊
type TerminationInfo struct {
	Reason    TerminationReason
	Message   string
	Timestamp time.Time
	Usage     ResourceUsage
}

// ProcessTerminator 進程終止器
type ProcessTerminator struct {
	gracePeriod time.Duration // 優雅關閉等待時間
}

// NewProcessTerminator 創建進程終止器
func NewProcessTerminator() *ProcessTerminator {
	return &ProcessTerminator{
		gracePeriod: 5 * time.Second,
	}
}

// TerminateGracefully 優雅終止進程
// 1. 發送 SIGTERM (Windows: TerminateProcess)
// 2. 等待 gracePeriod
// 3. 如果仍在運行，發送 SIGKILL
func (t *ProcessTerminator) TerminateGracefully(pid int) error {
	proc, err := os.FindProcess(pid)
	if err != nil {
		return fmt.Errorf("find process: %w", err)
	}

	// Windows 不支援 SIGTERM，直接使用 Kill
	// 在 Windows 上，proc.Signal(syscall.SIGTERM) 不會生效
	// 我們直接使用 proc.Kill() 來終止進程

	// 嘗試優雅終止（在 Unix 系統上會發送 SIGTERM）
	if err := proc.Signal(syscall.SIGTERM); err != nil {
		// 如果 SIGTERM 失敗（例如在 Windows 上），直接 Kill
		return proc.Kill()
	}

	// 等待進程結束
	done := make(chan error, 1)
	go func() {
		// 等待進程狀態變化
		_, err := proc.Wait()
		done <- err
	}()

	select {
	case <-time.After(t.gracePeriod):
		// 超時，強制終止
		return proc.Kill()
	case err := <-done:
		// 進程已結束
		return err
	}
}

// TerminateImmediately 立即終止進程
func (t *ProcessTerminator) TerminateImmediately(pid int) error {
	proc, err := os.FindProcess(pid)
	if err != nil {
		return fmt.Errorf("find process: %w", err)
	}

	return proc.Kill()
}

// ResourceLimitEnforcer 資源限制執行器
type ResourceLimitEnforcer struct {
	monitor    *ResourceMonitor
	terminator *ProcessTerminator
	limits     ResourceLimits
	pid        int

	terminated     bool
	terminationInfo *TerminationInfo
}

// NewResourceLimitEnforcer 創建資源限制執行器
func NewResourceLimitEnforcer(pid int, limits ResourceLimits, monitor *ResourceMonitor) *ResourceLimitEnforcer {
	return &ResourceLimitEnforcer{
		monitor:    monitor,
		terminator: NewProcessTerminator(),
		limits:     limits,
		pid:        pid,
	}
}

// CheckAndEnforce 檢查並執行資源限制
// 如果超限，返回 true 和終止資訊
func (e *ResourceLimitEnforcer) CheckAndEnforce() (terminated bool, info *TerminationInfo) {
	if e.terminated {
		return true, e.terminationInfo
	}

	// 檢查記憶體限制
	if err := e.monitor.CheckLimits(e.limits); err != nil {
		usage := e.monitor.GetCurrentUsage()

		info := &TerminationInfo{
			Reason:    TerminationMemoryLimit,
			Message:   err.Error(),
			Timestamp: time.Now(),
			Usage:     usage,
		}

		// 終止進程
		if termErr := e.terminator.TerminateGracefully(e.pid); termErr != nil {
			info.Message = fmt.Sprintf("%s (termination error: %v)", info.Message, termErr)
		}

		e.terminated = true
		e.terminationInfo = info
		return true, info
	}

	return false, nil
}

// IsTerminated 檢查是否已終止
func (e *ResourceLimitEnforcer) IsTerminated() bool {
	return e.terminated
}

// GetTerminationInfo 獲取終止資訊
func (e *ResourceLimitEnforcer) GetTerminationInfo() *TerminationInfo {
	return e.terminationInfo
}

// ForceTerminate 強制終止（用於用戶請求或其他原因）
func (e *ResourceLimitEnforcer) ForceTerminate(reason TerminationReason, message string) error {
	if e.terminated {
		return fmt.Errorf("process already terminated")
	}

	usage := e.monitor.GetCurrentUsage()

	info := &TerminationInfo{
		Reason:    reason,
		Message:   message,
		Timestamp: time.Now(),
		Usage:     usage,
	}

	// 終止進程
	if err := e.terminator.TerminateImmediately(e.pid); err != nil {
		return fmt.Errorf("terminate process: %w", err)
	}

	e.terminated = true
	e.terminationInfo = info
	return nil
}

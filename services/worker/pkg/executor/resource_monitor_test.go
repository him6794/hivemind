package executor

import (
	"os"
	"testing"
	"time"
)

func TestResourceLimits(t *testing.T) {
	t.Run("DefaultResourceLimits", func(t *testing.T) {
		limits := DefaultResourceLimits()

		if limits.MemoryMB != DefaultMemoryMB {
			t.Errorf("Expected MemoryMB=%d, got %d", DefaultMemoryMB, limits.MemoryMB)
		}

		if limits.Timeout != DefaultTimeout {
			t.Errorf("Expected Timeout=%v, got %v", DefaultTimeout, limits.Timeout)
		}

		if limits.MaxStackDepth != DefaultStackDepth {
			t.Errorf("Expected MaxStackDepth=%d, got %d", DefaultStackDepth, limits.MaxStackDepth)
		}

		if limits.MaxAllocations != DefaultMaxAllocs {
			t.Errorf("Expected MaxAllocations=%d, got %d", DefaultMaxAllocs, limits.MaxAllocations)
		}
	})

	t.Run("CustomResourceLimits", func(t *testing.T) {
		limits := ResourceLimits{
			MemoryMB:       1024,
			Timeout:        10 * time.Minute,
			MaxStackDepth:  2000,
			MaxAllocations: 2000000,
		}

		if limits.MemoryMB != 1024 {
			t.Errorf("Expected MemoryMB=1024, got %d", limits.MemoryMB)
		}

		if limits.Timeout != 10*time.Minute {
			t.Errorf("Expected Timeout=10m, got %v", limits.Timeout)
		}
	})
}

func TestMontyRunnerBuildArgs(t *testing.T) {
	runner := &MontyRunner{montyPath: "monty.exe"}

	tests := []struct {
		name       string
		scriptPath string
		limits     ResourceLimits
		wantArgs   []string
	}{
		{
			name:       "Default limits",
			scriptPath: "test.py",
			limits:     DefaultResourceLimits(),
			wantArgs: []string{
				"--memory-limit", "512",
				"--max-stack-depth", "1000",
				"--max-allocations", "1000000",
				"test.py",
			},
		},
		{
			name:       "Custom limits",
			scriptPath: "main.py",
			limits: ResourceLimits{
				MemoryMB:       256,
				MaxStackDepth:  500,
				MaxAllocations: 500000,
			},
			wantArgs: []string{
				"--memory-limit", "256",
				"--max-stack-depth", "500",
				"--max-allocations", "500000",
				"main.py",
			},
		},
		{
			name:       "Zero limits (disabled)",
			scriptPath: "app.py",
			limits: ResourceLimits{
				MemoryMB:       0,
				MaxStackDepth:  0,
				MaxAllocations: 0,
			},
			wantArgs: []string{"app.py"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			args := runner.buildMontyArgs(tt.scriptPath, tt.limits)

			if len(args) != len(tt.wantArgs) {
				t.Errorf("Expected %d args, got %d: %v", len(tt.wantArgs), len(args), args)
				return
			}

			for i, arg := range args {
				if arg != tt.wantArgs[i] {
					t.Errorf("Arg[%d]: expected %q, got %q", i, tt.wantArgs[i], arg)
				}
			}
		})
	}
}

func TestResourceMonitor(t *testing.T) {
	// 使用當前進程進行測試
	pid := os.Getpid()

	t.Run("CreateMonitor", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 1*time.Second, nil)

		if monitor.pid != int32(pid) {
			t.Errorf("Expected pid=%d, got %d", pid, monitor.pid)
		}

		if monitor.interval != 1*time.Second {
			t.Errorf("Expected interval=1s, got %v", monitor.interval)
		}
	})

	t.Run("StartAndStop", func(t *testing.T) {
		callCount := 0
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, func(usage ResourceUsage) {
			callCount++
		})

		err := monitor.Start()
		if err != nil {
			t.Fatalf("Start failed: %v", err)
		}

		// 等待幾次回調
		time.Sleep(350 * time.Millisecond)

		monitor.Stop()

		// 應該至少有 2-3 次回調
		if callCount < 2 {
			t.Errorf("Expected at least 2 callbacks, got %d", callCount)
		}
	})

	t.Run("GetCurrentUsage", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, nil)

		err := monitor.Start()
		if err != nil {
			t.Fatalf("Start failed: %v", err)
		}
		defer monitor.Stop()

		// 等待第一次數據收集
		time.Sleep(150 * time.Millisecond)

		usage := monitor.GetCurrentUsage()

		if usage.MemoryMB <= 0 {
			t.Errorf("Expected MemoryMB > 0, got %d", usage.MemoryMB)
		}

		if usage.CPUPercent < 0 {
			t.Errorf("Expected CPUPercent >= 0, got %.2f", usage.CPUPercent)
		}
	})

	t.Run("GetHistory", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, nil)

		err := monitor.Start()
		if err != nil {
			t.Fatalf("Start failed: %v", err)
		}
		defer monitor.Stop()

		// 等待收集幾次數據
		time.Sleep(350 * time.Millisecond)

		history := monitor.GetHistory()

		if len(history) < 2 {
			t.Errorf("Expected at least 2 history entries, got %d", len(history))
		}
	})

	t.Run("CheckLimits", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, nil)

		err := monitor.Start()
		if err != nil {
			t.Fatalf("Start failed: %v", err)
		}
		defer monitor.Stop()

		// 等待數據收集
		time.Sleep(150 * time.Millisecond)

		// 測試不會超限的限制
		limits := ResourceLimits{
			MemoryMB: 10000, // 10GB，不太可能超過
		}

		err = monitor.CheckLimits(limits)
		if err != nil {
			t.Errorf("CheckLimits should not fail with high limit: %v", err)
		}

		// 測試會超限的限制
		limits.MemoryMB = 1 // 1MB，肯定會超過

		err = monitor.CheckLimits(limits)
		if err == nil {
			t.Error("CheckLimits should fail with very low limit")
		}
	})

	t.Run("GetAverageUsage", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, nil)

		err := monitor.Start()
		if err != nil {
			t.Fatalf("Start failed: %v", err)
		}
		defer monitor.Stop()

		// 等待收集數據
		time.Sleep(350 * time.Millisecond)

		avgUsage := monitor.GetAverageUsage()

		if avgUsage.MemoryMB <= 0 {
			t.Errorf("Expected average MemoryMB > 0, got %d", avgUsage.MemoryMB)
		}
	})

	t.Run("GetPeakUsage", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, nil)

		err := monitor.Start()
		if err != nil {
			t.Fatalf("Start failed: %v", err)
		}
		defer monitor.Stop()

		// 等待收集數據
		time.Sleep(350 * time.Millisecond)

		peakUsage := monitor.GetPeakUsage()

		if peakUsage.MemoryMB <= 0 {
			t.Errorf("Expected peak MemoryMB > 0, got %d", peakUsage.MemoryMB)
		}
	})
}

func TestProcessTerminator(t *testing.T) {
	t.Run("CreateTerminator", func(t *testing.T) {
		terminator := NewProcessTerminator()

		if terminator.gracePeriod != 5*time.Second {
			t.Errorf("Expected gracePeriod=5s, got %v", terminator.gracePeriod)
		}
	})

	// 注意：實際的終止測試需要創建子進程，這裡只測試結構
}

func TestResourceLimitEnforcer(t *testing.T) {
	pid := os.Getpid()

	t.Run("CreateEnforcer", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 1*time.Second, nil)
		limits := DefaultResourceLimits()

		enforcer := NewResourceLimitEnforcer(pid, limits, monitor)

		if enforcer.pid != pid {
			t.Errorf("Expected pid=%d, got %d", pid, enforcer.pid)
		}

		if enforcer.IsTerminated() {
			t.Error("New enforcer should not be terminated")
		}
	})

	t.Run("CheckAndEnforce", func(t *testing.T) {
		monitor := NewResourceMonitor(pid, 100*time.Millisecond, nil)
		monitor.Start()
		defer monitor.Stop()

		// 等待數據收集
		time.Sleep(150 * time.Millisecond)

		// 使用不會超限的限制
		limits := ResourceLimits{
			MemoryMB: 10000,
		}

		enforcer := NewResourceLimitEnforcer(pid, limits, monitor)

		terminated, info := enforcer.CheckAndEnforce()

		if terminated {
			t.Errorf("Should not terminate with high limit, info: %v", info)
		}
	})
}

func TestTerminationInfo(t *testing.T) {
	t.Run("TerminationReasons", func(t *testing.T) {
		reasons := []TerminationReason{
			TerminationMemoryLimit,
			TerminationTimeout,
			TerminationUserRequest,
			TerminationError,
		}

		for _, reason := range reasons {
			if string(reason) == "" {
				t.Errorf("Termination reason should not be empty")
			}
		}
	})

	t.Run("CreateTerminationInfo", func(t *testing.T) {
		info := &TerminationInfo{
			Reason:    TerminationMemoryLimit,
			Message:   "Memory limit exceeded: 600 MB > 512 MB",
			Timestamp: time.Now(),
			Usage: ResourceUsage{
				CPUPercent: 50.0,
				MemoryMB:   600,
			},
		}

		if info.Reason != TerminationMemoryLimit {
			t.Errorf("Expected reason=%s, got %s", TerminationMemoryLimit, info.Reason)
		}

		if info.Usage.MemoryMB != 600 {
			t.Errorf("Expected MemoryMB=600, got %d", info.Usage.MemoryMB)
		}
	})
}

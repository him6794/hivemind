package executor

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

const (
	MontyExecutable  = `C:\Users\user\Desktop\monty\dist\monty.exe`
	DefaultTimeout   = 5 * time.Minute
	DefaultMemoryMB  = 512
	DefaultStackDepth = 1000
	DefaultMaxAllocs = 1000000
)

// ResourceLimits 定義任務執行的資源限制
type ResourceLimits struct {
	MemoryMB       int           // 記憶體限制 (MB)
	Timeout        time.Duration // 執行超時
	MaxStackDepth  int           // 堆疊深度限制
	MaxAllocations int           // 最大分配次數
}

// DefaultResourceLimits 返回預設的資源限制
func DefaultResourceLimits() ResourceLimits {
	return ResourceLimits{
		MemoryMB:       DefaultMemoryMB,
		Timeout:        DefaultTimeout,
		MaxStackDepth:  DefaultStackDepth,
		MaxAllocations: DefaultMaxAllocs,
	}
}

// MontyRunner 使用 monty.exe 執行 Python 腳本
type MontyRunner struct {
	montyPath string
}

// NewMontyRunner 創建新的 Monty 執行器
func NewMontyRunner() (*MontyRunner, error) {
	montyPath, err := resolveMontyPath()
	if err != nil {
		return nil, err
	}
	return &MontyRunner{montyPath: montyPath}, nil
}

func resolveMontyPath() (string, error) {
	candidates := []string{}
	if configured := strings.TrimSpace(os.Getenv("MONTY_EXECUTABLE")); configured != "" {
		candidates = append(candidates, configured)
	}
	candidates = append(candidates, repoMontyCandidates()...)
	candidates = append(candidates, MontyExecutable)

	for _, candidate := range candidates {
		if _, err := os.Stat(candidate); err == nil {
			return candidate, nil
		}
	}
	return "", fmt.Errorf("monty.exe not found; checked %s", strings.Join(candidates, ", "))
}

func repoMontyCandidates() []string {
	exeName := "monty"
	if runtime.GOOS == "windows" {
		exeName = "monty.exe"
	}

	candidates := []string{}
	if wd, err := os.Getwd(); err == nil {
		for dir := wd; ; dir = filepath.Dir(dir) {
			candidates = append(candidates,
				filepath.Join(dir, "executor-rs", "dist", exeName),
				filepath.Join(dir, "executor-rs", "target", "release", exeName),
				filepath.Join(dir, "executor-rs", exeName),
			)
			parent := filepath.Dir(dir)
			if parent == dir {
				break
			}
		}
	}
	return candidates
}

// ExecuteScript 執行 Python 腳本（使用預設資源限制）
func (r *MontyRunner) ExecuteScript(scriptPath string, workDir string) (stdout string, stderr string, err error) {
	return r.ExecuteWithLimits(scriptPath, workDir, DefaultResourceLimits())
}

// ExecuteWithLimits 執行 Python 腳本並應用資源限制
func (r *MontyRunner) ExecuteWithLimits(scriptPath string, workDir string, limits ResourceLimits) (stdout string, stderr string, err error) {
	ctx, cancel := context.WithTimeout(context.Background(), limits.Timeout)
	defer cancel()

	// 構建命令參數
	args := r.buildMontyArgs(scriptPath, limits)

	// 構建命令
	cmd := exec.CommandContext(ctx, r.montyPath, args...)
	cmd.Dir = workDir

	var stdoutBuf, stderrBuf bytes.Buffer
	cmd.Stdout = &stdoutBuf
	cmd.Stderr = &stderrBuf

	// 執行命令
	err = cmd.Run()

	stdout = stdoutBuf.String()
	stderr = stderrBuf.String()

	// 檢查超時
	if ctx.Err() == context.DeadlineExceeded {
		return stdout, stderr, fmt.Errorf("execution timeout after %v", limits.Timeout)
	}

	return stdout, stderr, err
}

// buildMontyArgs 構建 Monty 命令列參數
func (r *MontyRunner) buildMontyArgs(scriptPath string, limits ResourceLimits) []string {
	args := []string{}

	// 記憶體限制
	if limits.MemoryMB > 0 {
		args = append(args, "--memory-limit", fmt.Sprintf("%d", limits.MemoryMB))
	}

	// 堆疊深度限制
	if limits.MaxStackDepth > 0 {
		args = append(args, "--max-stack-depth", fmt.Sprintf("%d", limits.MaxStackDepth))
	}

	// 最大分配次數
	if limits.MaxAllocations > 0 {
		args = append(args, "--max-allocations", fmt.Sprintf("%d", limits.MaxAllocations))
	}

	// 腳本路徑
	args = append(args, scriptPath)

	return args
}

// FindExecutableScript 查找可執行的 Python 腳本
func FindExecutableScript(workDir string) (string, error) {
	// 優先級順序
	candidates := []string{
		"main.py",
		"run.py",
		"app.py",
		"start.py",
		"__main__.py",
	}

	for _, name := range candidates {
		path := filepath.Join(workDir, name)
		if _, err := os.Stat(path); err == nil {
			return path, nil
		}
	}

	// 如果沒有找到，搜索第一個 .py 文件
	entries, err := os.ReadDir(workDir)
	if err != nil {
		return "", fmt.Errorf("read workdir: %w", err)
	}

	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".py") {
			return filepath.Join(workDir, entry.Name()), nil
		}
	}

	return "", fmt.Errorf("no executable Python script found in %s", workDir)
}

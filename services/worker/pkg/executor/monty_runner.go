//go:build legacy_executor

package executor

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const (
	MontyExecutable  = `C:\Users\user\Desktop\monty\dist\monty.exe`
	ExecutionTimeout = 5 * time.Minute
)

// MontyRunner 使用 monty.exe 執行 Python 腳本
type MontyRunner struct {
	montyPath string
}

// NewMontyRunner 創建新的 Monty 執行器
func NewMontyRunner() (*MontyRunner, error) {
	// 檢查 monty.exe 是否存在
	if _, err := os.Stat(MontyExecutable); err != nil {
		return nil, fmt.Errorf("monty.exe not found at %s: %w", MontyExecutable, err)
	}
	return &MontyRunner{montyPath: MontyExecutable}, nil
}

// ExecuteScript 執行 Python 腳本
func (r *MontyRunner) ExecuteScript(scriptPath string, workDir string) (stdout string, stderr string, err error) {
	ctx, cancel := context.WithTimeout(context.Background(), ExecutionTimeout)
	defer cancel()

	// 構建命令
	cmd := exec.CommandContext(ctx, r.montyPath, scriptPath)
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
		return stdout, stderr, fmt.Errorf("execution timeout after %v", ExecutionTimeout)
	}

	return stdout, stderr, err
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

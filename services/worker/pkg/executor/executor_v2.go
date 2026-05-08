package executor

import (
	"fmt"
	"io"
	"net"
	"net/http"
	neturl "net/url"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

const (
	MaxDownloadSize = 100 * 1024 * 1024 // 100MB
	MonitorInterval = 5 * time.Second    // 資源監控間隔
)

// TaskResult 任務執行結果
type TaskResult struct {
	TaskID          string
	Success         bool
	Stdout          string
	Stderr          string
	Logs            []string
	ResultZip       []byte
	Error           error
	ResourceUsage   *ResourceUsage      // 平均資源使用
	PeakUsage       *ResourceUsage      // 峰值資源使用
	TerminationInfo *TerminationInfo    // 終止資訊（如果被終止）
}

// ExecuteTaskOptions 任務執行選項
type ExecuteTaskOptions struct {
	TaskID       string
	DownloadURL  string
	Limits       ResourceLimits
	OnProgress   func(ResourceUsage) // 資源使用回調
}

// ExecuteTask 執行完整的任務生命周期（使用預設資源限制）
func ExecuteTask(taskID string, downloadURL string) (*TaskResult, error) {
	return ExecuteTaskWithOptions(ExecuteTaskOptions{
		TaskID:      taskID,
		DownloadURL: downloadURL,
		Limits:      DefaultResourceLimits(),
	})
}

// ExecuteTaskWithLimits 執行任務並應用自定義資源限制
func ExecuteTaskWithLimits(taskID string, downloadURL string, limits ResourceLimits) (*TaskResult, error) {
	return ExecuteTaskWithOptions(ExecuteTaskOptions{
		TaskID:      taskID,
		DownloadURL: downloadURL,
		Limits:      limits,
	})
}

// ExecuteTaskWithOptions 執行完整的任務生命周期（完整選項）
// 1. 下載任務 ZIP
// 2. 安全解壓
// 3. 使用 monty.exe 執行（帶資源限制）
// 4. 即時監控資源使用
// 5. 打包結果
func ExecuteTaskWithOptions(opts ExecuteTaskOptions) (*TaskResult, error) {
	result := &TaskResult{
		TaskID: opts.TaskID,
		Logs:   []string{},
	}

	// 創建臨時工作目錄
	tempDir, err := os.MkdirTemp("", fmt.Sprintf("task_%s_*", opts.TaskID))
	if err != nil {
		result.Error = fmt.Errorf("create temp dir: %w", err)
		return result, result.Error
	}
	defer os.RemoveAll(tempDir)

	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Created temp dir: %s", time.Now().Format("15:04:05"), tempDir))
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Resource limits: Memory=%dMB, Timeout=%v",
		time.Now().Format("15:04:05"), opts.Limits.MemoryMB, opts.Limits.Timeout))

	// 1. 下載任務 ZIP
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Downloading task from: %s", time.Now().Format("15:04:05"), opts.DownloadURL))
	zipPath, fileSize, err := downloadFileToTemp(opts.DownloadURL, tempDir)
	if err != nil {
		result.Error = fmt.Errorf("download task: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}
	defer os.Remove(zipPath)
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Downloaded %d bytes", time.Now().Format("15:04:05"), fileSize))

	// 2. 安全解壓
	workDir := filepath.Join(tempDir, "work")
	if err := os.MkdirAll(workDir, 0700); err != nil {
		result.Error = fmt.Errorf("create work dir: %w", err)
		return result, result.Error
	}

	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Extracting ZIP...", time.Now().Format("15:04:05")))
	if err := SafeExtractZipFromFile(zipPath, workDir); err != nil {
		result.Error = fmt.Errorf("extract zip: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Extracted to: %s", time.Now().Format("15:04:05"), workDir))

	// 3. 查找可執行腳本
	scriptPath, err := FindExecutableScript(workDir)
	if err != nil {
		result.Error = fmt.Errorf("find script: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Found script: %s", time.Now().Format("15:04:05"), filepath.Base(scriptPath)))

	// 4. 使用 monty.exe 執行（帶資源監控）
	runner, err := NewMontyRunner()
	if err != nil {
		result.Error = fmt.Errorf("create monty runner: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}

	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Executing with monty.exe (with resource monitoring)...", time.Now().Format("15:04:05")))

	// 執行並監控
	stdout, stderr, execErr, monitor := executeWithMonitoring(runner, scriptPath, workDir, opts.Limits, opts.OnProgress, result)

	result.Stdout = stdout
	result.Stderr = stderr

	// 收集資源使用統計
	if monitor != nil {
		avgUsage := monitor.GetAverageUsage()
		peakUsage := monitor.GetPeakUsage()
		result.ResourceUsage = &avgUsage
		result.PeakUsage = &peakUsage

		result.Logs = append(result.Logs, fmt.Sprintf("[%s] Resource usage - Avg: CPU=%.2f%%, Memory=%dMB | Peak: CPU=%.2f%%, Memory=%dMB",
			time.Now().Format("15:04:05"),
			avgUsage.CPUPercent, avgUsage.MemoryMB,
			peakUsage.CPUPercent, peakUsage.MemoryMB))
	}

	if execErr != nil {
		result.Success = false
		result.Error = execErr
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] Execution failed: %v", time.Now().Format("15:04:05"), execErr))
		if stderr != "" {
			result.Logs = append(result.Logs, fmt.Sprintf("[%s] STDERR: %s", time.Now().Format("15:04:05"), stderr))
		}
	} else {
		result.Success = true
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] Execution completed successfully", time.Now().Format("15:04:05")))
	}

	if stdout != "" {
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] STDOUT: %s", time.Now().Format("15:04:05"), stdout))
	}

	// 5. 打包結果
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Packaging results...", time.Now().Format("15:04:05")))
	resultZip, err := PackageResults(workDir, opts.TaskID, result.Success, result.Logs)
	if err != nil {
		result.Error = fmt.Errorf("package results: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}
	result.ResultZip = resultZip
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Result ZIP created: %d bytes", time.Now().Format("15:04:05"), len(resultZip)))

	return result, nil
}

// executeWithMonitoring 執行腳本並監控資源使用
func executeWithMonitoring(
	runner *MontyRunner,
	scriptPath string,
	workDir string,
	limits ResourceLimits,
	onProgress func(ResourceUsage),
	result *TaskResult,
) (stdout, stderr string, err error, monitor *ResourceMonitor) {

	// 啟動 Monty 進程
	cmd := runner.buildCommand(scriptPath, workDir, limits)

	// 啟動進程
	if err := cmd.Start(); err != nil {
		return "", "", fmt.Errorf("start process: %w", err), nil
	}

	pid := cmd.Process.Pid

	// 創建資源監控器
	monitor = NewResourceMonitor(pid, MonitorInterval, func(usage ResourceUsage) {
		// 記錄資源使用
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] Resource: CPU=%.2f%%, Memory=%dMB",
			time.Now().Format("15:04:05"), usage.CPUPercent, usage.MemoryMB))

		// 回調
		if onProgress != nil {
			onProgress(usage)
		}
	})

	// 啟動監控
	if err := monitor.Start(); err != nil {
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] WARNING: Failed to start resource monitor: %v",
			time.Now().Format("15:04:05"), err))
	} else {
		defer monitor.Stop()

		// 創建資源限制執行器
		enforcer := NewResourceLimitEnforcer(pid, limits, monitor)

		// 啟動資源限制檢查
		go func() {
			ticker := time.NewTicker(MonitorInterval)
			defer ticker.Stop()

			for range ticker.C {
				if terminated, info := enforcer.CheckAndEnforce(); terminated {
					result.TerminationInfo = info
					result.Logs = append(result.Logs, fmt.Sprintf("[%s] TERMINATED: %s - %s",
						time.Now().Format("15:04:05"), info.Reason, info.Message))
					break
				}
			}
		}()
	}

	// 等待進程結束
	err = cmd.Wait()

	// 讀取輸出
	if cmd.Stdout != nil {
		if buf, ok := cmd.Stdout.(*os.File); ok {
			data, _ := io.ReadAll(buf)
			stdout = string(data)
		}
	}
	if cmd.Stderr != nil {
		if buf, ok := cmd.Stderr.(*os.File); ok {
			data, _ := io.ReadAll(buf)
			stderr = string(data)
		}
	}

	return stdout, stderr, err, monitor
}

// buildCommand 構建執行命令（MontyRunner 的輔助方法）
func (r *MontyRunner) buildCommand(scriptPath string, workDir string, limits ResourceLimits) *exec.Cmd {
	args := r.buildMontyArgs(scriptPath, limits)
	cmd := exec.Command(r.montyPath, args...)
	cmd.Dir = workDir
	return cmd
}

// 包級別的 HTTP Client（復用連接）
var httpClient = &http.Client{
	Timeout: 5 * time.Minute,
	Transport: &http.Transport{
		MaxIdleConns:        100,
		MaxIdleConnsPerHost: 10,
		IdleConnTimeout:     90 * time.Second,
	},
	CheckRedirect: func(req *http.Request, via []*http.Request) error {
		if err := validateURL(req.URL.String()); err != nil {
			return err
		}
		if len(via) >= 10 {
			return fmt.Errorf("too many redirects")
		}
		return nil
	},
}

// validateURL 驗證 URL 安全性（防止 SSRF）
func validateURL(urlStr string) error {
	parsedURL, err := neturl.Parse(urlStr)
	if err != nil {
		return fmt.Errorf("invalid url: %w", err)
	}

	if parsedURL.Scheme != "http" && parsedURL.Scheme != "https" {
		return fmt.Errorf("unsupported protocol: %s", parsedURL.Scheme)
	}

	host := parsedURL.Hostname()

	// 測試環境允許 localhost
	if host == "127.0.0.1" {
		port := parsedURL.Port()
		if port != "" && port != "80" && port != "443" {
			return nil
		}
	}

	if host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0" {
		return fmt.Errorf("access to localhost is forbidden")
	}

	ip := net.ParseIP(host)
	if ip != nil {
		if ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() {
			return fmt.Errorf("access to private IP is forbidden: %s", host)
		}
		if ip.String() == "169.254.169.254" {
			return fmt.Errorf("access to metadata service is forbidden")
		}
	}

	return nil
}

// downloadFileToTemp 下載文件到臨時文件
func downloadFileToTemp(url string, tempDir string) (string, int64, error) {
	if err := validateURL(url); err != nil {
		return "", 0, err
	}

	resp, err := httpClient.Get(url)
	if err != nil {
		return "", 0, fmt.Errorf("http get: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", 0, fmt.Errorf("http status: %d", resp.StatusCode)
	}

	if resp.ContentLength > MaxDownloadSize {
		return "", 0, fmt.Errorf("file too large: %d bytes (max: %d)",
			resp.ContentLength, MaxDownloadSize)
	}

	tmpFile, err := os.CreateTemp(tempDir, "download_*.zip")
	if err != nil {
		return "", 0, fmt.Errorf("create temp file: %w", err)
	}
	tmpPath := tmpFile.Name()

	limitedReader := io.LimitReader(resp.Body, MaxDownloadSize+1)
	written, err := io.Copy(tmpFile, limitedReader)
	tmpFile.Close()

	if err != nil {
		os.Remove(tmpPath)
		return "", 0, fmt.Errorf("download failed: %w", err)
	}

	if written > MaxDownloadSize {
		os.Remove(tmpPath)
		return "", 0, fmt.Errorf("file exceeds size limit: %d MB", MaxDownloadSize/1024/1024)
	}

	return tmpPath, written, nil
}

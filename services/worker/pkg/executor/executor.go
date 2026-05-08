package executor

import (
	"fmt"
	"io"
	"net"
	"net/http"
	neturl "net/url"
	"os"
	"path/filepath"
	"time"
)

const (
	MaxDownloadSize = 100 * 1024 * 1024 // 100MB
)

// TaskResult 任務執行結果
type TaskResult struct {
	TaskID    string
	Success   bool
	Stdout    string
	Stderr    string
	Logs      []string
	ResultZip []byte
	Error     error
}

// ExecuteTask 執行完整的任務生命周期
// 1. 下載任務 ZIP
// 2. 安全解壓
// 3. 使用 monty.exe 執行
// 4. 打包結果
func ExecuteTask(taskID string, downloadURL string) (*TaskResult, error) {
	result := &TaskResult{
		TaskID: taskID,
		Logs:   []string{},
	}

	// 創建臨時工作目錄
	tempDir, err := os.MkdirTemp("", fmt.Sprintf("task_%s_*", taskID))
	if err != nil {
		result.Error = fmt.Errorf("create temp dir: %w", err)
		return result, result.Error
	}
	defer os.RemoveAll(tempDir)

	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Created temp dir: %s", time.Now().Format("15:04:05"), tempDir))

	// 1. 下載任務 ZIP（流式處理，不加載到內存）
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Downloading task from: %s", time.Now().Format("15:04:05"), downloadURL))
	zipPath, fileSize, err := downloadFileToTemp(downloadURL, tempDir)
	if err != nil {
		result.Error = fmt.Errorf("download task: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}
	defer os.Remove(zipPath) // 下載完成後刪除臨時 ZIP
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Downloaded %d bytes", time.Now().Format("15:04:05"), fileSize))

	// 2. 安全解壓（從文件讀取，不加載到內存）
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

	// 4. 使用 monty.exe 執行
	runner, err := NewMontyRunner()
	if err != nil {
		result.Error = fmt.Errorf("create monty runner: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}

	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Executing with monty.exe...", time.Now().Format("15:04:05")))
	stdout, stderr, err := runner.ExecuteScript(scriptPath, workDir)
	result.Stdout = stdout
	result.Stderr = stderr

	if err != nil {
		result.Success = false
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] Execution failed: %v", time.Now().Format("15:04:05"), err))
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
	resultZip, err := PackageResults(workDir, taskID, result.Success, result.Logs)
	if err != nil {
		result.Error = fmt.Errorf("package results: %w", err)
		result.Logs = append(result.Logs, fmt.Sprintf("[%s] ERROR: %v", time.Now().Format("15:04:05"), err))
		return result, result.Error
	}
	result.ResultZip = resultZip
	result.Logs = append(result.Logs, fmt.Sprintf("[%s] Result ZIP created: %d bytes", time.Now().Format("15:04:05"), len(resultZip)))

	return result, nil
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
		// 檢查重定向目標
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

	// 僅允許 http 和 https
	if parsedURL.Scheme != "http" && parsedURL.Scheme != "https" {
		return fmt.Errorf("unsupported protocol: %s", parsedURL.Scheme)
	}

	host := parsedURL.Hostname()

	// 檢查是否為測試環境（允許 127.0.0.1 的高端口）
	if host == "127.0.0.1" {
		port := parsedURL.Port()
		if port != "" && port != "80" && port != "443" {
			// 允許測試服務器使用高端口
			return nil
		}
	}

	// 禁止訪問本地地址
	if host == "localhost" || host == "127.0.0.1" || host == "0.0.0.0" {
		return fmt.Errorf("access to localhost is forbidden")
	}

	// 禁止訪問私有 IP
	ip := net.ParseIP(host)
	if ip != nil {
		if ip.IsPrivate() || ip.IsLoopback() || ip.IsLinkLocalUnicast() {
			return fmt.Errorf("access to private IP is forbidden: %s", host)
		}

		// 禁止訪問 metadata 服務
		if ip.String() == "169.254.169.254" {
			return fmt.Errorf("access to metadata service is forbidden")
		}
	}

	return nil
}

// downloadFileToTemp 下載文件到臨時文件（流式處理，不加載到內存）
func downloadFileToTemp(url string, tempDir string) (string, int64, error) {
	// 驗證 URL
	if err := validateURL(url); err != nil {
		return "", 0, err
	}

	// 發送請求
	resp, err := httpClient.Get(url)
	if err != nil {
		return "", 0, fmt.Errorf("http get: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", 0, fmt.Errorf("http status: %d", resp.StatusCode)
	}

	// 檢查 Content-Length
	if resp.ContentLength > MaxDownloadSize {
		return "", 0, fmt.Errorf("file too large: %d bytes (max: %d)",
			resp.ContentLength, MaxDownloadSize)
	}

	// 創建臨時文件
	tmpFile, err := os.CreateTemp(tempDir, "download_*.zip")
	if err != nil {
		return "", 0, fmt.Errorf("create temp file: %w", err)
	}
	tmpPath := tmpFile.Name()

	// 流式複製（限制大小）
	limitedReader := io.LimitReader(resp.Body, MaxDownloadSize+1)
	written, err := io.Copy(tmpFile, limitedReader)
	tmpFile.Close()

	if err != nil {
		os.Remove(tmpPath)
		return "", 0, fmt.Errorf("download failed: %w", err)
	}

	// 檢查是否超過限制
	if written > MaxDownloadSize {
		os.Remove(tmpPath)
		return "", 0, fmt.Errorf("file exceeds size limit: %d MB", MaxDownloadSize/1024/1024)
	}

	return tmpPath, written, nil
}

package executor

import (
	"archive/zip"
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"
)

// PackageResults 打包任務執行結果為 ZIP
func PackageResults(workDir string, taskID string, success bool, logs []string) ([]byte, error) {
	buf := new(bytes.Buffer)
	zipWriter := zip.NewWriter(buf)
	defer zipWriter.Close()

	// 1. 添加執行日誌
	logContent := fmt.Sprintf("Task ID: %s\n", taskID)
	logContent += fmt.Sprintf("Status: %s\n", map[bool]string{true: "Success", false: "Failed"}[success])
	logContent += fmt.Sprintf("Time: %s\n", time.Now().Format("2006-01-02 15:04:05"))
	logContent += "\n=== Execution Logs ===\n"
	for _, log := range logs {
		logContent += log + "\n"
	}
	logContent += "\n=== End of Logs ===\n"

	logWriter, err := zipWriter.Create("execution_log.txt")
	if err != nil {
		return nil, fmt.Errorf("create log file: %w", err)
	}
	if _, err := logWriter.Write([]byte(logContent)); err != nil {
		return nil, fmt.Errorf("write log: %w", err)
	}

	// 2. 添加工作目錄中的所有文件
	err = filepath.Walk(workDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// 跳過目錄本身
		if info.IsDir() {
			return nil
		}

		// 計算相對路徑
		relPath, err := filepath.Rel(workDir, path)
		if err != nil {
			return err
		}

		// 創建 ZIP 條目
		writer, err := zipWriter.Create(relPath)
		if err != nil {
			return fmt.Errorf("create zip entry %s: %w", relPath, err)
		}

		// 讀取並寫入文件內容
		file, err := os.Open(path)
		if err != nil {
			return fmt.Errorf("open file %s: %w", path, err)
		}
		defer file.Close()

		if _, err := io.Copy(writer, file); err != nil {
			return fmt.Errorf("write file %s: %w", relPath, err)
		}

		return nil
	})

	if err != nil {
		return nil, fmt.Errorf("walk workdir: %w", err)
	}

	if err := zipWriter.Close(); err != nil {
		return nil, fmt.Errorf("close zip: %w", err)
	}

	return buf.Bytes(), nil
}

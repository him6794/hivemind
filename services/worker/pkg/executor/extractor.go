package executor

import (
	"archive/zip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

const (
	MaxExtractedSize = 1024 * 1024 * 1024 // 1GB
	MaxFileCount     = 10000
	DirPerm          = 0700 // 僅所有者可訪問
	FilePerm         = 0600 // 僅所有者可讀寫
)

// SafeExtractZipFromFile 從文件安全解壓 ZIP，防止 ZIP 炸彈和路徑遍歷攻擊
func SafeExtractZipFromFile(zipPath string, destDir string) error {
	// 打開 ZIP 文件
	zipFile, err := os.Open(zipPath)
	if err != nil {
		return fmt.Errorf("open zip file: %w", err)
	}
	defer zipFile.Close()

	// 獲取文件大小
	stat, err := zipFile.Stat()
	if err != nil {
		return fmt.Errorf("stat zip file: %w", err)
	}

	reader, err := zip.NewReader(zipFile, stat.Size())
	if err != nil {
		return fmt.Errorf("open zip: %w", err)
	}

	// 檢查文件數量
	if len(reader.File) > MaxFileCount {
		return fmt.Errorf("too many files: %d exceeds limit %d",
			len(reader.File), MaxFileCount)
	}

	// 單次遍歷：檢查大小並解壓
	var totalSize int64
	var compressedSize int64

	for _, file := range reader.File {
		// 檢查符號鏈接
		if file.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("symbolic links are not allowed: %s", file.Name)
		}

		totalSize += int64(file.UncompressedSize64)
		compressedSize += int64(file.CompressedSize64)

		// 檢查 ZIP 炸彈
		if totalSize > MaxExtractedSize {
			ratio := float64(totalSize) / float64(compressedSize)
			return fmt.Errorf("zip bomb detected: uncompressed size %d exceeds limit %d (ratio: %.1fx)",
				totalSize, MaxExtractedSize, ratio)
		}

		// 立即解壓
		if err := extractFile(file, destDir); err != nil {
			return fmt.Errorf("extract file %s: %w", file.Name, err)
		}
	}

	return nil
}

func extractFile(file *zip.File, destDir string) error {
	// 防止路徑遍歷攻擊
	filePath := filepath.Join(destDir, file.Name)
	cleanDest := filepath.Clean(destDir)
	cleanPath := filepath.Clean(filePath)

	if !strings.HasPrefix(cleanPath, cleanDest) {
		return fmt.Errorf("illegal file path: %s", file.Name)
	}

	// 檢查路徑中是否包含 ..
	if strings.Contains(file.Name, "..") {
		return fmt.Errorf("illegal file path with ..: %s", file.Name)
	}

	if file.FileInfo().IsDir() {
		return os.MkdirAll(filePath, DirPerm)
	}

	// 創建父目錄
	if err := os.MkdirAll(filepath.Dir(filePath), DirPerm); err != nil {
		return err
	}

	// 解壓文件
	rc, err := file.Open()
	if err != nil {
		return err
	}
	defer rc.Close()

	// 移除執行權限，僅保留讀寫
	perm := file.Mode() & 0666
	if perm == 0 {
		perm = FilePerm
	}

	outFile, err := os.OpenFile(filePath, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, perm)
	if err != nil {
		return err
	}
	defer outFile.Close()

	_, err = io.Copy(outFile, rc)
	return err
}

//go:build windows

/*
HiveMind Worker Installer
自動安裝 Docker、下載 Worker 程式、連接 VPN
*/

package main

import (
	"archive/zip"
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"golang.org/x/sys/windows"
)

const (
	VERSION             = "1.0.0"
	UPDATE_SERVER       = "https://hivemind.jack0916295614.workers.dev"
	VPN_API_URL         = "https://hivemind.justin0711.com/api/vpn/join"
	DOCKER_DOWNLOAD_URL = "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe"
	INSTALL_DIR         = "C:\\HiveMind"
)

// UpdateManifest 更新清單結構
type UpdateManifest struct {
	Latest   string                       `json:"latest"`
	Versions map[string]VersionInfo       `json:"versions"`
}

type VersionInfo struct {
	ReleaseDate string     `json:"release_date"`
	Artifacts   []Artifact `json:"artifacts"`
}

type Artifact struct {
	OS          string `json:"os"`
	Arch        string `json:"arch"`
	Filename    string `json:"filename"`
	DownloadURL string `json:"download_url"`
	Size        int64  `json:"size"`
	SHA256      string `json:"sha256"`
}

// 顏色輸出
func printColor(color string, format string, args ...interface{}) {
	colors := map[string]string{
		"red":    "\033[31m",
		"green":  "\033[32m",
		"yellow": "\033[33m",
		"blue":   "\033[34m",
		"cyan":   "\033[36m",
		"reset":  "\033[0m",
	}
	fmt.Printf(colors[color]+format+colors["reset"], args...)
}

func printStep(step int, total int, msg string) {
	printColor("cyan", "\n[%d/%d] %s\n", step, total, msg)
}

func printSuccess(msg string) {
	printColor("green", "  ✓ %s\n", msg)
}

func printError(msg string) {
	printColor("red", "  ✗ %s\n", msg)
}

func printWarning(msg string) {
	printColor("yellow", "  ⚠ %s\n", msg)
}

func printInfo(msg string) {
	fmt.Printf("  → %s\n", msg)
}

// isAdmin 檢查是否以管理員身份執行
func isAdmin() bool {
	_, err := os.Open("\\\\.\\PHYSICALDRIVE0")
	return err == nil
}

// runAsAdmin 以管理員身份重新執行
func runAsAdmin() error {
	exe, err := os.Executable()
	if err != nil {
		return err
	}

	cwd, err := os.Getwd()
	if err != nil {
		return err
	}

	verb := "runas"
	verbPtr, _ := windows.UTF16PtrFromString(verb)
	exePtr, _ := windows.UTF16PtrFromString(exe)
	cwdPtr, _ := windows.UTF16PtrFromString(cwd)
	argPtr, _ := windows.UTF16PtrFromString(strings.Join(os.Args[1:], " "))

	err = windows.ShellExecute(0, verbPtr, exePtr, argPtr, cwdPtr, windows.SW_NORMAL)
	return err
}

// checkDockerInstalled 檢查 Docker 是否已安裝
func checkDockerInstalled() bool {
	cmd := exec.Command("docker", "--version")
	err := cmd.Run()
	return err == nil
}

// checkDockerRunning 檢查 Docker 是否正在運行
func checkDockerRunning() bool {
	cmd := exec.Command("docker", "info")
	err := cmd.Run()
	return err == nil
}

// downloadFile 下載檔案並顯示進度
func downloadFile(url string, destPath string, showProgress bool) error {
	resp, err := http.Get(url)
	if err != nil {
		return fmt.Errorf("下載失敗: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("HTTP 錯誤: %d", resp.StatusCode)
	}

	out, err := os.Create(destPath)
	if err != nil {
		return fmt.Errorf("建立檔案失敗: %v", err)
	}
	defer out.Close()

	totalSize := resp.ContentLength
	downloaded := int64(0)
	buffer := make([]byte, 32*1024)

	for {
		n, err := resp.Body.Read(buffer)
		if n > 0 {
			out.Write(buffer[:n])
			downloaded += int64(n)

			if showProgress && totalSize > 0 {
				percent := float64(downloaded) / float64(totalSize) * 100
				fmt.Printf("\r  下載進度: %.1f%% (%d/%d MB)", percent, downloaded/1024/1024, totalSize/1024/1024)
			}
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("讀取失敗: %v", err)
		}
	}

	if showProgress {
		fmt.Println()
	}
	return nil
}

// installDocker 安裝 Docker Desktop
func installDocker() error {
	printStep(1, 4, "安裝 Docker Desktop")

	if checkDockerInstalled() {
		printSuccess("Docker 已安裝")
		
		if !checkDockerRunning() {
			printWarning("Docker 未運行，嘗試啟動...")
			exec.Command("cmd", "/c", "start", "", "Docker Desktop").Run()
			
			// 等待 Docker 啟動
			for i := 0; i < 60; i++ {
				time.Sleep(2 * time.Second)
				if checkDockerRunning() {
					printSuccess("Docker 已啟動")
					return nil
				}
				fmt.Printf("\r  等待 Docker 啟動... %ds", (i+1)*2)
			}
			fmt.Println()
			printWarning("Docker 啟動超時，請手動啟動 Docker Desktop")
		} else {
			printSuccess("Docker 正在運行")
		}
		return nil
	}

	printInfo("正在下載 Docker Desktop 安裝程式...")

	// 建立臨時目錄
	tempDir := os.TempDir()
	installerPath := filepath.Join(tempDir, "DockerDesktopInstaller.exe")

	// 下載 Docker Desktop 安裝程式
	err := downloadFile(DOCKER_DOWNLOAD_URL, installerPath, true)
	if err != nil {
		return fmt.Errorf("下載 Docker 安裝程式失敗: %v", err)
	}

	printInfo("正在安裝 Docker Desktop (這可能需要幾分鐘)...")

	// 靜默安裝 Docker
	cmd := exec.Command(installerPath, "install", "--quiet", "--accept-license")
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	err = cmd.Run()
	if err != nil {
		return fmt.Errorf("安裝 Docker 失敗: %v", err)
	}

	// 清理安裝檔案
	os.Remove(installerPath)

	printSuccess("Docker Desktop 安裝完成")
	printWarning("請重新啟動電腦以完成 Docker 安裝，然後再次運行此安裝程式")
	
	return nil
}

// getUpdateInfo 獲取最新版本資訊
func getUpdateInfo() (*Artifact, string, error) {
	resp, err := http.Get(UPDATE_SERVER + "/worker/manifest")
	if err != nil {
		return nil, "", fmt.Errorf("獲取更新資訊失敗: %v", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, "", fmt.Errorf("讀取回應失敗: %v", err)
	}

	var manifest UpdateManifest
	if err := json.Unmarshal(body, &manifest); err != nil {
		return nil, "", fmt.Errorf("解析清單失敗: %v", err)
	}

	if manifest.Latest == "" {
		return nil, "", fmt.Errorf("找不到最新版本")
	}

	versionInfo, ok := manifest.Versions[manifest.Latest]
	if !ok {
		return nil, "", fmt.Errorf("找不到版本 %s 的資訊", manifest.Latest)
	}

	// 尋找對應平台的 artifact
	osName := runtime.GOOS
	archName := runtime.GOARCH

	for _, artifact := range versionInfo.Artifacts {
		if artifact.OS == osName && artifact.Arch == archName {
			return &artifact, manifest.Latest, nil
		}
	}

	return nil, "", fmt.Errorf("找不到適用於 %s/%s 的安裝包", osName, archName)
}

// downloadAndExtractWorker 下載並解壓 Worker
func downloadAndExtractWorker() error {
	printStep(2, 4, "下載 HiveMind Worker")

	artifact, version, err := getUpdateInfo()
	if err != nil {
		return err
	}

	printInfo(fmt.Sprintf("最新版本: %s", version))
	printInfo(fmt.Sprintf("檔案大小: %.2f MB", float64(artifact.Size)/1024/1024))

	// 建立安裝目錄
	if err := os.MkdirAll(INSTALL_DIR, 0755); err != nil {
		return fmt.Errorf("建立安裝目錄失敗: %v", err)
	}

	// 下載檔案
	tempFile := filepath.Join(os.TempDir(), artifact.Filename)
	printInfo("正在下載...")

	err = downloadFile(artifact.DownloadURL, tempFile, true)
	if err != nil {
		return err
	}

	// 驗證 SHA256
	if artifact.SHA256 != "" {
		printInfo("驗證檔案完整性...")
		fileData, err := os.ReadFile(tempFile)
		if err != nil {
			return fmt.Errorf("讀取檔案失敗: %v", err)
		}

		hash := sha256.Sum256(fileData)
		hashStr := hex.EncodeToString(hash[:])

		if hashStr != artifact.SHA256 {
			os.Remove(tempFile)
			return fmt.Errorf("SHA256 驗證失敗")
		}
		printSuccess("SHA256 驗證通過")
	}

	// 解壓縮
	printInfo("正在解壓縮...")
	if strings.HasSuffix(artifact.Filename, ".zip") {
		err = unzip(tempFile, INSTALL_DIR)
	} else {
		// 如果是單一執行檔，直接複製
		destPath := filepath.Join(INSTALL_DIR, artifact.Filename)
		err = copyFile(tempFile, destPath)
	}

	if err != nil {
		return fmt.Errorf("解壓縮失敗: %v", err)
	}

	// 清理臨時檔案
	os.Remove(tempFile)

	printSuccess(fmt.Sprintf("Worker 已安裝到 %s", INSTALL_DIR))
	return nil
}

// unzip 解壓縮 ZIP 檔案
func unzip(src, dest string) error {
	r, err := zip.OpenReader(src)
	if err != nil {
		return err
	}
	defer r.Close()

	for _, f := range r.File {
		fpath := filepath.Join(dest, f.Name)

		// 安全檢查：防止 zip slip 攻擊
		if !strings.HasPrefix(fpath, filepath.Clean(dest)+string(os.PathSeparator)) {
			return fmt.Errorf("非法的檔案路徑: %s", fpath)
		}

		if f.FileInfo().IsDir() {
			os.MkdirAll(fpath, os.ModePerm)
			continue
		}

		if err := os.MkdirAll(filepath.Dir(fpath), os.ModePerm); err != nil {
			return err
		}

		outFile, err := os.OpenFile(fpath, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, f.Mode())
		if err != nil {
			return err
		}

		rc, err := f.Open()
		if err != nil {
			outFile.Close()
			return err
		}

		_, err = io.Copy(outFile, rc)
		outFile.Close()
		rc.Close()

		if err != nil {
			return err
		}
	}
	return nil
}

// copyFile 複製檔案
func copyFile(src, dest string) error {
	source, err := os.Open(src)
	if err != nil {
		return err
	}
	defer source.Close()

	destination, err := os.Create(dest)
	if err != nil {
		return err
	}
	defer destination.Close()

	_, err = io.Copy(destination, source)
	return err
}

// setupVPN 設定並連接 VPN
func setupVPN() error {
	printStep(3, 4, "設定 VPN 連接")

	vpnDir := filepath.Join(INSTALL_DIR, "vpn")
	if err := os.MkdirAll(vpnDir, 0755); err != nil {
		return fmt.Errorf("建立 VPN 目錄失敗: %v", err)
	}

	configPath := filepath.Join(vpnDir, "wg0.conf")
	vpnExePath := filepath.Join(vpnDir, "hivemind-vpn.exe")

	// 檢查 VPN 執行檔是否存在
	if _, err := os.Stat(vpnExePath); os.IsNotExist(err) {
		printInfo("正在下載 VPN 程式...")
		// 從主安裝包或另外下載
		// 這裡假設 VPN 程式包含在 Worker 安裝包中
		// 或者需要單獨下載
		srcVpn := filepath.Join(INSTALL_DIR, "hivemind-vpn.exe")
		if _, err := os.Stat(srcVpn); err == nil {
			copyFile(srcVpn, vpnExePath)
		}
	}

	// 請求 VPN 配置
	printInfo("正在請求 VPN 配置...")
	err := requestVPNConfig(configPath)
	if err != nil {
		return fmt.Errorf("獲取 VPN 配置失敗: %v", err)
	}

	// 複製 wintun.dll
	wintunSrc := filepath.Join(INSTALL_DIR, "wintun.dll")
	wintunDst := filepath.Join(vpnDir, "wintun.dll")
	if _, err := os.Stat(wintunSrc); err == nil {
		copyFile(wintunSrc, wintunDst)
	}

	// 啟動 VPN 並測試連接
	printInfo("正在連接 VPN...")
	
	// 檢查 VPN 執行檔
	if _, err := os.Stat(vpnExePath); os.IsNotExist(err) {
		printWarning("VPN 執行檔不存在，請手動安裝 VPN")
		return nil
	}

	cmd := exec.Command(vpnExePath, configPath, "--auto-retry")
	cmd.Dir = vpnDir
	err = cmd.Start()
	if err != nil {
		return fmt.Errorf("啟動 VPN 失敗: %v", err)
	}

	// 等待 VPN 連接
	time.Sleep(5 * time.Second)

	// 測試連接
	if pingTest("10.0.0.1") {
		printSuccess("VPN 連接成功")
		
		// 安裝開機自啟動
		installVPNAutoStart(vpnExePath, configPath)
	} else {
		printWarning("VPN 連接測試失敗，請檢查網路設定")
	}

	return nil
}

// requestVPNConfig 請求 VPN 配置
func requestVPNConfig(configPath string) error {
	hostname, _ := os.Hostname()
	clientName := fmt.Sprintf("worker-%s-%d", hostname, os.Getpid())

	reqBody, _ := json.Marshal(map[string]string{
		"client_name": clientName,
	})

	client := &http.Client{Timeout: 15 * time.Second}
	resp, err := client.Post(VPN_API_URL, "application/json", bytes.NewBuffer(reqBody))
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}

	var result map[string]interface{}
	if err := json.Unmarshal(body, &result); err != nil {
		return err
	}

	if success, ok := result["success"].(bool); !ok || !success {
		errMsg := "未知錯誤"
		if e, ok := result["error"].(string); ok {
			errMsg = e
		}
		return fmt.Errorf("API 錯誤: %s", errMsg)
	}

	config, ok := result["config"].(string)
	if !ok || config == "" {
		return fmt.Errorf("配置為空")
	}

	return os.WriteFile(configPath, []byte(config), 0600)
}

// pingTest 測試 ping
func pingTest(ip string) bool {
	cmd := exec.Command("ping", "-n", "3", "-w", "1000", ip)
	output, err := cmd.CombinedOutput()
	if err != nil {
		return false
	}
	return strings.Contains(string(output), "TTL=") || strings.Contains(string(output), "ttl=")
}

// installVPNAutoStart 安裝 VPN 開機自啟動
func installVPNAutoStart(vpnExePath, configPath string) {
	printInfo("設定 VPN 開機自啟動...")

	taskName := "HiveMind-VPN-AutoStart"
	
	// 先刪除舊任務
	exec.Command("schtasks", "/delete", "/tn", taskName, "/f").Run()

	// 建立新任務
	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s" "%s" --auto-retry`, vpnExePath, configPath),
		"/sc", "onstart",
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)
	
	if err := cmd.Run(); err != nil {
		printWarning("設定開機自啟動失敗")
	} else {
		printSuccess("VPN 開機自啟動已設定")
	}
}

// setupWorkerAutoStart 設定 Worker 開機自啟動
func setupWorkerAutoStart() error {
	printStep(4, 4, "設定 Worker 開機自啟動")

	workerExe := filepath.Join(INSTALL_DIR, "worker_node.exe")
	
	// 檢查 Worker 執行檔
	if _, err := os.Stat(workerExe); os.IsNotExist(err) {
		// 嘗試尋找其他可能的執行檔名稱
		matches, _ := filepath.Glob(filepath.Join(INSTALL_DIR, "*.exe"))
		for _, m := range matches {
			if strings.Contains(filepath.Base(m), "worker") {
				workerExe = m
				break
			}
		}
	}

	if _, err := os.Stat(workerExe); os.IsNotExist(err) {
		printWarning("找不到 Worker 執行檔")
		return nil
	}

	taskName := "HiveMind-Worker-AutoStart"
	
	// 先刪除舊任務
	exec.Command("schtasks", "/delete", "/tn", taskName, "/f").Run()

	// 建立新任務
	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s"`, workerExe),
		"/sc", "onstart",
		"/delay", "0001:00",  // 延遲 1 分鐘啟動（等 VPN 連接）
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("設定開機自啟動失敗: %v", err)
	}

	printSuccess("Worker 開機自啟動已設定")
	printInfo(fmt.Sprintf("Worker 位置: %s", workerExe))

	return nil
}

// createDesktopShortcut 建立桌面捷徑
func createDesktopShortcut() {
	printInfo("建立桌面捷徑...")

	userProfile := os.Getenv("USERPROFILE")
	desktopPath := filepath.Join(userProfile, "Desktop")
	
	workerExe := filepath.Join(INSTALL_DIR, "worker_node.exe")
	shortcutPath := filepath.Join(desktopPath, "HiveMind Worker.lnk")

	// 使用 PowerShell 建立捷徑
	script := fmt.Sprintf(`
$WshShell = New-Object -comObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut("%s")
$Shortcut.TargetPath = "%s"
$Shortcut.WorkingDirectory = "%s"
$Shortcut.Description = "HiveMind Worker Node"
$Shortcut.Save()
`, shortcutPath, workerExe, INSTALL_DIR)

	cmd := exec.Command("powershell", "-Command", script)
	if err := cmd.Run(); err != nil {
		printWarning("建立桌面捷徑失敗")
	} else {
		printSuccess("桌面捷徑已建立")
	}
}

func main() {
	fmt.Println("╔════════════════════════════════════════╗")
	fmt.Println("║     HiveMind Worker Installer v" + VERSION + "     ║")
	fmt.Println("╚════════════════════════════════════════╝")
	fmt.Println()

	// 檢查管理員權限
	if !isAdmin() {
		printWarning("需要管理員權限，正在請求提權...")
		if err := runAsAdmin(); err != nil {
			printError("無法取得管理員權限")
			fmt.Println("\n請右鍵點擊安裝程式，選擇「以系統管理員身份執行」")
			fmt.Println("\n按 Enter 鍵退出...")
			fmt.Scanln()
			os.Exit(1)
		}
		os.Exit(0)
	}

	printSuccess("已獲得管理員權限")

	var hasError bool

	// 步驟 1: 安裝 Docker
	if err := installDocker(); err != nil {
		printError(err.Error())
		hasError = true
	}

	// 檢查 Docker 是否可用
	if !checkDockerRunning() {
		printWarning("Docker 未運行，部分功能可能無法使用")
	}

	// 步驟 2: 下載 Worker
	if err := downloadAndExtractWorker(); err != nil {
		printError(err.Error())
		hasError = true
	}

	// 步驟 3: 設定 VPN
	if err := setupVPN(); err != nil {
		printError(err.Error())
		hasError = true
	}

	// 步驟 4: 設定開機自啟動
	if err := setupWorkerAutoStart(); err != nil {
		printError(err.Error())
		hasError = true
	}

	// 建立桌面捷徑
	createDesktopShortcut()

	fmt.Println()
	fmt.Println("════════════════════════════════════════")
	if hasError {
		printWarning("安裝完成，但有部分步驟失敗")
	} else {
		printSuccess("HiveMind Worker 安裝完成！")
	}
	fmt.Println("════════════════════════════════════════")
	fmt.Printf("\n安裝目錄: %s\n", INSTALL_DIR)
	fmt.Println("\n按 Enter 鍵退出...")
	fmt.Scanln()
}

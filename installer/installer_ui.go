//go:build windows

/*
HiveMind Worker Installer with UI
帶有圖形介面的安裝程式
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
	"sync"
	"time"

	"github.com/webview/webview_go"
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
	Latest   string                 `json:"latest"`
	Versions map[string]VersionInfo `json:"versions"`
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

// Installer 安裝器主結構
type Installer struct {
	w          webview.WebView
	mutex      sync.Mutex
	installing bool
}

// 全域安裝器實例
var installer *Installer

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

	return windows.ShellExecute(0, verbPtr, exePtr, argPtr, cwdPtr, windows.SW_NORMAL)
}

// updateUI 更新 UI 狀態
func (inst *Installer) updateUI(js string) {
	inst.mutex.Lock()
	defer inst.mutex.Unlock()
	inst.w.Eval(js)
}

// setProgress 設定進度
func (inst *Installer) setProgress(percent int, message string) {
	js := fmt.Sprintf(`setProgress(%d, "%s")`, percent, escapeJS(message))
	inst.updateUI(js)
}

// addLog 新增日誌
func (inst *Installer) addLog(logType, message string) {
	js := fmt.Sprintf(`addLog("%s", "%s")`, logType, escapeJS(message))
	inst.updateUI(js)
}

// setStepStatus 設定步驟狀態
func (inst *Installer) setStepStatus(step int, status string) {
	js := fmt.Sprintf(`setStepStatus(%d, "%s")`, step, status)
	inst.updateUI(js)
}

// escapeJS 轉義 JavaScript 字串
func escapeJS(s string) string {
	s = strings.ReplaceAll(s, "\\", "\\\\")
	s = strings.ReplaceAll(s, "\"", "\\\"")
	s = strings.ReplaceAll(s, "\n", "\\n")
	s = strings.ReplaceAll(s, "\r", "")
	return s
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

// downloadFileWithProgress 下載檔案並回報進度
func (inst *Installer) downloadFileWithProgress(url, destPath string, baseProgress, maxProgress int) error {
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

			if totalSize > 0 {
				progress := baseProgress + int(float64(downloaded)/float64(totalSize)*float64(maxProgress-baseProgress))
				sizeMB := float64(downloaded) / 1024 / 1024
				totalMB := float64(totalSize) / 1024 / 1024
				inst.setProgress(progress, fmt.Sprintf("下載中... %.1f / %.1f MB", sizeMB, totalMB))
			}
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("讀取失敗: %v", err)
		}
	}

	return nil
}

// installDocker 安裝 Docker Desktop
func (inst *Installer) installDocker() error {
	inst.setStepStatus(1, "running")
	inst.addLog("info", "檢查 Docker 安裝狀態...")

	if checkDockerInstalled() {
		inst.addLog("success", "Docker 已安裝")

		// 設定 Docker 開機自啟動
		inst.addLog("info", "設定 Docker 開機自啟動...")
		inst.setDockerAutoStart()

		if !checkDockerRunning() {
			inst.addLog("warning", "Docker 未運行，嘗試啟動...")
			exec.Command("cmd", "/c", "start", "", "Docker Desktop").Run()

			for i := 0; i < 30; i++ {
				time.Sleep(2 * time.Second)
				inst.setProgress(5+i, fmt.Sprintf("等待 Docker 啟動... %ds", (i+1)*2))
				if checkDockerRunning() {
					inst.addLog("success", "Docker 已啟動")
					inst.setStepStatus(1, "done")
					return nil
				}
			}
			inst.addLog("warning", "Docker 啟動超時，請稍後手動啟動")
		} else {
			inst.addLog("success", "Docker 正在運行")
		}
		inst.setStepStatus(1, "done")
		return nil
	}

	inst.addLog("info", "正在下載 Docker Desktop 安裝程式...")
	inst.setProgress(5, "下載 Docker Desktop...")

	tempDir := os.TempDir()
	installerPath := filepath.Join(tempDir, "DockerDesktopInstaller.exe")

	err := inst.downloadFileWithProgress(DOCKER_DOWNLOAD_URL, installerPath, 5, 25)
	if err != nil {
		inst.setStepStatus(1, "error")
		return fmt.Errorf("下載 Docker 安裝程式失敗: %v", err)
	}

	inst.addLog("info", "正在安裝 Docker Desktop (這可能需要幾分鐘)...")
	inst.setProgress(30, "安裝 Docker Desktop...")

	cmd := exec.Command(installerPath, "install", "--quiet", "--accept-license")
	err = cmd.Run()
	if err != nil {
		inst.setStepStatus(1, "error")
		return fmt.Errorf("安裝 Docker 失敗: %v", err)
	}

	os.Remove(installerPath)

	// 設定 Docker 開機自啟動
	inst.addLog("info", "設定 Docker 開機自啟動...")
	inst.setDockerAutoStart()

	inst.addLog("success", "Docker Desktop 安裝完成")
	inst.addLog("warning", "請重新啟動電腦以完成 Docker 安裝")
	inst.setStepStatus(1, "done")

	return nil
}

// setDockerAutoStart 設定 Docker 開機自啟動
func (inst *Installer) setDockerAutoStart() {
	// 方法 1: 透過 Registry 設定開機啟動
	regPath := `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
	dockerPath := `"C:\Program Files\Docker\Docker\Docker Desktop.exe"`
	
	cmd := exec.Command("reg", "add", regPath, "/v", "Docker Desktop", "/t", "REG_SZ", "/d", dockerPath, "/f")
	if err := cmd.Run(); err != nil {
		// 方法 2: 嘗試透過 Docker Desktop 設定
		settingsPath := filepath.Join(os.Getenv("APPDATA"), "Docker", "settings.json")
		if data, err := os.ReadFile(settingsPath); err == nil {
			var settings map[string]interface{}
			if json.Unmarshal(data, &settings) == nil {
				settings["autoStart"] = true
				settings["startAtLogin"] = true
				if newData, err := json.MarshalIndent(settings, "", "  "); err == nil {
					os.WriteFile(settingsPath, newData, 0644)
				}
			}
		}
	}

	inst.addLog("success", "Docker 開機自啟動已設定")
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
func (inst *Installer) downloadAndExtractWorker() error {
	inst.setStepStatus(2, "running")
	inst.addLog("info", "檢查最新版本...")

	artifact, version, err := getUpdateInfo()
	if err != nil {
		inst.setStepStatus(2, "error")
		return err
	}

	inst.addLog("info", fmt.Sprintf("最新版本: %s", version))
	inst.addLog("info", fmt.Sprintf("檔案大小: %.2f MB", float64(artifact.Size)/1024/1024))

	if err := os.MkdirAll(INSTALL_DIR, 0755); err != nil {
		inst.setStepStatus(2, "error")
		return fmt.Errorf("建立安裝目錄失敗: %v", err)
	}

	tempFile := filepath.Join(os.TempDir(), artifact.Filename)
	inst.addLog("info", "正在下載 Worker...")

	err = inst.downloadFileWithProgress(artifact.DownloadURL, tempFile, 35, 55)
	if err != nil {
		inst.setStepStatus(2, "error")
		return err
	}

	if artifact.SHA256 != "" {
		inst.addLog("info", "驗證檔案完整性...")
		fileData, err := os.ReadFile(tempFile)
		if err != nil {
			inst.setStepStatus(2, "error")
			return fmt.Errorf("讀取檔案失敗: %v", err)
		}

		hash := sha256.Sum256(fileData)
		hashStr := hex.EncodeToString(hash[:])

		if hashStr != artifact.SHA256 {
			os.Remove(tempFile)
			inst.setStepStatus(2, "error")
			return fmt.Errorf("SHA256 驗證失敗")
		}
		inst.addLog("success", "SHA256 驗證通過")
	}

	inst.addLog("info", "正在解壓縮...")
	inst.setProgress(60, "解壓縮中...")

	if strings.HasSuffix(artifact.Filename, ".zip") {
		err = unzip(tempFile, INSTALL_DIR)
	} else {
		destPath := filepath.Join(INSTALL_DIR, artifact.Filename)
		err = copyFile(tempFile, destPath)
	}

	if err != nil {
		inst.setStepStatus(2, "error")
		return fmt.Errorf("解壓縮失敗: %v", err)
	}

	os.Remove(tempFile)

	inst.addLog("success", fmt.Sprintf("Worker 已安裝到 %s", INSTALL_DIR))
	inst.setStepStatus(2, "done")
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
func (inst *Installer) setupVPN() error {
	inst.setStepStatus(3, "running")
	inst.addLog("info", "設定 VPN...")

	vpnDir := filepath.Join(INSTALL_DIR, "vpn")
	if err := os.MkdirAll(vpnDir, 0755); err != nil {
		inst.setStepStatus(3, "error")
		return fmt.Errorf("建立 VPN 目錄失敗: %v", err)
	}

	configPath := filepath.Join(vpnDir, "wg0.conf")
	vpnExePath := filepath.Join(vpnDir, "hivemind-vpn.exe")

	// 複製 VPN 相關檔案
	srcVpn := filepath.Join(INSTALL_DIR, "hivemind-vpn.exe")
	if _, err := os.Stat(srcVpn); err == nil {
		copyFile(srcVpn, vpnExePath)
	}

	wintunSrc := filepath.Join(INSTALL_DIR, "wintun.dll")
	wintunDst := filepath.Join(vpnDir, "wintun.dll")
	if _, err := os.Stat(wintunSrc); err == nil {
		copyFile(wintunSrc, wintunDst)
	}

	inst.addLog("info", "正在請求 VPN 配置...")
	inst.setProgress(70, "請求 VPN 配置...")

	err := requestVPNConfig(configPath)
	if err != nil {
		inst.addLog("warning", fmt.Sprintf("獲取 VPN 配置失敗: %v", err))
		inst.setStepStatus(3, "warning")
		return nil
	}

	inst.addLog("success", "VPN 配置已取得")

	// 檢查 VPN 執行檔
	if _, err := os.Stat(vpnExePath); os.IsNotExist(err) {
		inst.addLog("warning", "VPN 執行檔不存在")
		inst.setStepStatus(3, "warning")
		return nil
	}

	inst.addLog("info", "正在連接 VPN...")
	inst.setProgress(75, "連接 VPN...")

	cmd := exec.Command(vpnExePath, configPath, "--auto-retry")
	cmd.Dir = vpnDir
	err = cmd.Start()
	if err != nil {
		inst.addLog("warning", fmt.Sprintf("啟動 VPN 失敗: %v", err))
		inst.setStepStatus(3, "warning")
		return nil
	}

	time.Sleep(5 * time.Second)

	if pingTest("10.0.0.1") {
		inst.addLog("success", "VPN 連接成功")
		inst.installVPNAutoStart(vpnExePath, configPath)
		inst.setStepStatus(3, "done")
	} else {
		inst.addLog("warning", "VPN 連接測試失敗")
		inst.setStepStatus(3, "warning")
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
func (inst *Installer) installVPNAutoStart(vpnExePath, configPath string) {
	inst.addLog("info", "設定 VPN 開機自啟動...")

	taskName := "HiveMind-VPN-AutoStart"
	exec.Command("schtasks", "/delete", "/tn", taskName, "/f").Run()

	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s" "%s" --auto-retry`, vpnExePath, configPath),
		"/sc", "onstart",
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)

	if err := cmd.Run(); err != nil {
		inst.addLog("warning", "設定 VPN 開機自啟動失敗")
	} else {
		inst.addLog("success", "VPN 開機自啟動已設定")
	}
}

// setupWorkerAutoStart 設定 Worker 開機自啟動
func (inst *Installer) setupWorkerAutoStart() error {
	inst.setStepStatus(4, "running")
	inst.addLog("info", "設定 Worker 開機自啟動...")
	inst.setProgress(85, "設定開機自啟動...")

	workerExe := filepath.Join(INSTALL_DIR, "worker_node.exe")

	if _, err := os.Stat(workerExe); os.IsNotExist(err) {
		matches, _ := filepath.Glob(filepath.Join(INSTALL_DIR, "*.exe"))
		for _, m := range matches {
			if strings.Contains(strings.ToLower(filepath.Base(m)), "worker") {
				workerExe = m
				break
			}
		}
	}

	if _, err := os.Stat(workerExe); os.IsNotExist(err) {
		inst.addLog("warning", "找不到 Worker 執行檔")
		inst.setStepStatus(4, "warning")
		return nil
	}

	taskName := "HiveMind-Worker-AutoStart"
	exec.Command("schtasks", "/delete", "/tn", taskName, "/f").Run()

	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s"`, workerExe),
		"/sc", "onstart",
		"/delay", "0001:00",
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)

	if err := cmd.Run(); err != nil {
		inst.addLog("warning", "設定 Worker 開機自啟動失敗")
		inst.setStepStatus(4, "warning")
		return nil
	}

	inst.addLog("success", "Worker 開機自啟動已設定")
	inst.addLog("info", fmt.Sprintf("Worker 位置: %s", workerExe))
	inst.setStepStatus(4, "done")

	return nil
}

// createDesktopShortcut 建立桌面捷徑
func (inst *Installer) createDesktopShortcut() {
	inst.addLog("info", "建立桌面捷徑...")
	inst.setProgress(95, "建立桌面捷徑...")

	userProfile := os.Getenv("USERPROFILE")
	desktopPath := filepath.Join(userProfile, "Desktop")
	workerExe := filepath.Join(INSTALL_DIR, "worker_node.exe")
	shortcutPath := filepath.Join(desktopPath, "HiveMind Worker.lnk")

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
		inst.addLog("warning", "建立桌面捷徑失敗")
	} else {
		inst.addLog("success", "桌面捷徑已建立")
	}
}

// startInstallation 開始安裝
func (inst *Installer) startInstallation() {
	inst.mutex.Lock()
	if inst.installing {
		inst.mutex.Unlock()
		return
	}
	inst.installing = true
	inst.mutex.Unlock()

	go func() {
		defer func() {
			inst.mutex.Lock()
			inst.installing = false
			inst.mutex.Unlock()
		}()

		hasError := false

		// 步驟 1: 安裝 Docker
		if err := inst.installDocker(); err != nil {
			inst.addLog("error", err.Error())
			hasError = true
		}

		// 步驟 2: 下載 Worker
		if err := inst.downloadAndExtractWorker(); err != nil {
			inst.addLog("error", err.Error())
			hasError = true
		}

		// 步驟 3: 設定 VPN
		if err := inst.setupVPN(); err != nil {
			inst.addLog("error", err.Error())
			hasError = true
		}

		// 步驟 4: 設定開機自啟動
		if err := inst.setupWorkerAutoStart(); err != nil {
			inst.addLog("error", err.Error())
			hasError = true
		}

		// 建立桌面捷徑
		inst.createDesktopShortcut()

		inst.setProgress(100, "完成")

		if hasError {
			inst.updateUI(`showComplete(false)`)
		} else {
			inst.updateUI(`showComplete(true)`)
		}
	}()
}

// HTML UI 模板
const htmlTemplate = `
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>HiveMind Worker 安裝程式</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }
        body {
            font-family: 'Segoe UI', 'Microsoft JhengHei', sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            color: #fff;
            min-height: 100vh;
            padding: 20px;
        }
        .container {
            max-width: 600px;
            margin: 0 auto;
        }
        .header {
            text-align: center;
            padding: 20px 0;
            border-bottom: 1px solid rgba(255,255,255,0.1);
            margin-bottom: 20px;
        }
        .header h1 {
            font-size: 28px;
            margin-bottom: 5px;
            background: linear-gradient(90deg, #00d9ff, #00ff88);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .header .version {
            color: #888;
            font-size: 14px;
        }
        .steps {
            margin-bottom: 20px;
        }
        .step {
            display: flex;
            align-items: center;
            padding: 12px 15px;
            background: rgba(255,255,255,0.05);
            border-radius: 8px;
            margin-bottom: 8px;
            transition: all 0.3s;
        }
        .step.running {
            background: rgba(0,217,255,0.1);
            border-left: 3px solid #00d9ff;
        }
        .step.done {
            background: rgba(0,255,136,0.1);
            border-left: 3px solid #00ff88;
        }
        .step.error {
            background: rgba(255,68,68,0.1);
            border-left: 3px solid #ff4444;
        }
        .step.warning {
            background: rgba(255,187,0,0.1);
            border-left: 3px solid #ffbb00;
        }
        .step-icon {
            width: 24px;
            height: 24px;
            margin-right: 12px;
            display: flex;
            align-items: center;
            justify-content: center;
        }
        .step-icon.pending { color: #666; }
        .step-icon.running { color: #00d9ff; }
        .step-icon.done { color: #00ff88; }
        .step-icon.error { color: #ff4444; }
        .step-icon.warning { color: #ffbb00; }
        .spinner {
            width: 20px;
            height: 20px;
            border: 2px solid rgba(0,217,255,0.3);
            border-top-color: #00d9ff;
            border-radius: 50%;
            animation: spin 1s linear infinite;
        }
        @keyframes spin {
            to { transform: rotate(360deg); }
        }
        .progress-container {
            margin-bottom: 20px;
        }
        .progress-bar {
            height: 8px;
            background: rgba(255,255,255,0.1);
            border-radius: 4px;
            overflow: hidden;
            margin-bottom: 8px;
        }
        .progress-fill {
            height: 100%;
            background: linear-gradient(90deg, #00d9ff, #00ff88);
            width: 0%;
            transition: width 0.3s;
        }
        .progress-text {
            font-size: 13px;
            color: #aaa;
            text-align: center;
        }
        .log-container {
            background: rgba(0,0,0,0.3);
            border-radius: 8px;
            padding: 15px;
            max-height: 200px;
            overflow-y: auto;
            margin-bottom: 20px;
        }
        .log-entry {
            font-size: 12px;
            padding: 4px 0;
            border-bottom: 1px solid rgba(255,255,255,0.05);
        }
        .log-entry:last-child { border-bottom: none; }
        .log-entry.info { color: #aaa; }
        .log-entry.success { color: #00ff88; }
        .log-entry.warning { color: #ffbb00; }
        .log-entry.error { color: #ff4444; }
        .log-entry::before {
            margin-right: 8px;
        }
        .log-entry.info::before { content: '→'; }
        .log-entry.success::before { content: '✓'; }
        .log-entry.warning::before { content: '⚠'; }
        .log-entry.error::before { content: '✗'; }
        .btn {
            width: 100%;
            padding: 15px;
            border: none;
            border-radius: 8px;
            font-size: 16px;
            font-weight: bold;
            cursor: pointer;
            transition: all 0.3s;
        }
        .btn-primary {
            background: linear-gradient(90deg, #00d9ff, #00ff88);
            color: #1a1a2e;
        }
        .btn-primary:hover {
            transform: translateY(-2px);
            box-shadow: 0 5px 20px rgba(0,217,255,0.3);
        }
        .btn-primary:disabled {
            background: #444;
            color: #888;
            cursor: not-allowed;
            transform: none;
            box-shadow: none;
        }
        .complete-message {
            text-align: center;
            padding: 30px;
            display: none;
        }
        .complete-message.show { display: block; }
        .complete-message.success h2 { color: #00ff88; }
        .complete-message.error h2 { color: #ff4444; }
        .complete-message p {
            color: #aaa;
            margin-top: 10px;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🐝 HiveMind Worker</h1>
            <div class="version">安裝程式 v` + VERSION + `</div>
        </div>

        <div id="installPanel">
            <div class="steps">
                <div class="step" id="step1">
                    <div class="step-icon pending" id="icon1">○</div>
                    <div>安裝 Docker Desktop</div>
                </div>
                <div class="step" id="step2">
                    <div class="step-icon pending" id="icon2">○</div>
                    <div>下載 HiveMind Worker</div>
                </div>
                <div class="step" id="step3">
                    <div class="step-icon pending" id="icon3">○</div>
                    <div>設定 VPN 連接</div>
                </div>
                <div class="step" id="step4">
                    <div class="step-icon pending" id="icon4">○</div>
                    <div>設定開機自啟動</div>
                </div>
            </div>

            <div class="progress-container">
                <div class="progress-bar">
                    <div class="progress-fill" id="progressFill"></div>
                </div>
                <div class="progress-text" id="progressText">準備就緒</div>
            </div>

            <div class="log-container" id="logContainer">
                <div class="log-entry info">點擊「開始安裝」按鈕開始...</div>
            </div>

            <button class="btn btn-primary" id="installBtn" onclick="startInstall()">
                開始安裝
            </button>
        </div>

        <div class="complete-message" id="completeMessage">
            <h2 id="completeTitle"></h2>
            <p id="completeText"></p>
        </div>
    </div>

    <script>
        function setProgress(percent, message) {
            document.getElementById('progressFill').style.width = percent + '%';
            document.getElementById('progressText').innerText = message;
        }

        function addLog(type, message) {
            const container = document.getElementById('logContainer');
            const entry = document.createElement('div');
            entry.className = 'log-entry ' + type;
            entry.innerText = message;
            container.appendChild(entry);
            container.scrollTop = container.scrollHeight;
        }

        function setStepStatus(step, status) {
            const stepEl = document.getElementById('step' + step);
            const iconEl = document.getElementById('icon' + step);
            
            stepEl.className = 'step ' + status;
            iconEl.className = 'step-icon ' + status;
            
            if (status === 'pending') {
                iconEl.innerHTML = '○';
            } else if (status === 'running') {
                iconEl.innerHTML = '<div class="spinner"></div>';
            } else if (status === 'done') {
                iconEl.innerHTML = '✓';
            } else if (status === 'error') {
                iconEl.innerHTML = '✗';
            } else if (status === 'warning') {
                iconEl.innerHTML = '⚠';
            }
        }

        function showComplete(success) {
            document.getElementById('installPanel').style.display = 'none';
            const completeEl = document.getElementById('completeMessage');
            completeEl.classList.add('show');
            completeEl.classList.add(success ? 'success' : 'error');
            
            document.getElementById('completeTitle').innerText = 
                success ? '✓ 安裝完成！' : '⚠ 安裝完成（部分失敗）';
            document.getElementById('completeText').innerText = 
                success ? '您可以關閉此視窗。Worker 將在下次開機時自動啟動。' 
                       : '部分步驟未能完成，請查看日誌了解詳情。';
        }

        function startInstall() {
            document.getElementById('installBtn').disabled = true;
            document.getElementById('installBtn').innerText = '安裝中...';
            document.getElementById('logContainer').innerHTML = '';
            window.startInstallation();
        }
    </script>
</body>
</html>
`

func main() {
	// 檢查管理員權限
	if !isAdmin() {
		runAsAdmin()
		os.Exit(0)
	}

	// 建立 WebView
	w := webview.New(false)
	defer w.Destroy()

	installer = &Installer{w: w}

	w.SetTitle("HiveMind Worker 安裝程式")
	w.SetSize(650, 600, webview.HintNone)

	// 綁定 Go 函數到 JavaScript
	w.Bind("startInstallation", func() {
		installer.startInstallation()
	})

	w.SetHtml(htmlTemplate)
	w.Run()
}

//go:build windows

/*
HiveMind Worker Installer with Fyne UI
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
	"syscall"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"
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

// Installer main struct
type Installer struct {
	app         fyne.App
	window      fyne.Window
	progressBar *widget.ProgressBar
	statusLabel *widget.Label
	logList     *widget.List
	logs        []string
	installBtn  *widget.Button
	steps       []*widget.Label
	stepIcons   []*widget.Label
	installing  bool
}

// isAdmin checks if running as administrator
func isAdmin() bool {
	_, err := os.Open("\\\\.\\PHYSICALDRIVE0")
	return err == nil
}

// runAsAdmin restarts with admin privileges
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

// NewInstaller creates a new installer
func NewInstaller() *Installer {
	inst := &Installer{
		logs: make([]string, 0),
	}

	inst.app = app.New()
	inst.app.Settings().SetTheme(theme.DarkTheme())
	inst.window = inst.app.NewWindow("HiveMind Worker Installer v" + VERSION)
	inst.window.Resize(fyne.NewSize(600, 500))
	inst.window.CenterOnScreen()

	return inst
}

// addLog adds a log entry
func (inst *Installer) addLog(message string) {
	inst.logs = append(inst.logs, message)
	if inst.logList != nil {
		inst.logList.Refresh()
		inst.logList.ScrollToBottom()
	}
}

// setProgress sets the progress bar
func (inst *Installer) setProgress(percent float64, message string) {
	if inst.progressBar != nil {
		inst.progressBar.SetValue(percent / 100)
	}
	if inst.statusLabel != nil {
		inst.statusLabel.SetText(message)
	}
}

// setStepStatus sets step status icon
func (inst *Installer) setStepStatus(step int, status string) {
	if step < 1 || step > len(inst.stepIcons) {
		return
	}
	icon := inst.stepIcons[step-1]
	switch status {
	case "pending":
		icon.SetText("○")
	case "running":
		icon.SetText("◐")
	case "done":
		icon.SetText("✓")
	case "error":
		icon.SetText("✗")
	case "warning":
		icon.SetText("⚠")
	}
}

// buildUI builds the UI
func (inst *Installer) buildUI() fyne.CanvasObject {
	// Title
	title := widget.NewLabelWithStyle("🐝 HiveMind Worker Installer",
		fyne.TextAlignCenter, fyne.TextStyle{Bold: true})

	// Steps list
	stepNames := []string{
		"Install Docker Desktop",
		"Download HiveMind Worker",
		"Setup VPN Connection",
		"Configure Auto-Start",
	}

	inst.steps = make([]*widget.Label, 4)
	inst.stepIcons = make([]*widget.Label, 4)

	stepsContainer := container.NewVBox()
	for i, name := range stepNames {
		inst.stepIcons[i] = widget.NewLabel("○")
		inst.steps[i] = widget.NewLabel(name)
		stepRow := container.NewHBox(inst.stepIcons[i], inst.steps[i])
		stepsContainer.Add(stepRow)
	}

	// Progress bar
	inst.progressBar = widget.NewProgressBar()
	inst.statusLabel = widget.NewLabel("Ready")
	inst.statusLabel.Alignment = fyne.TextAlignCenter

	progressContainer := container.NewVBox(
		inst.progressBar,
		inst.statusLabel,
	)

	// Log list
	inst.logList = widget.NewList(
		func() int { return len(inst.logs) },
		func() fyne.CanvasObject { return widget.NewLabel("") },
		func(id widget.ListItemID, obj fyne.CanvasObject) {
			obj.(*widget.Label).SetText(inst.logs[id])
		},
	)
	inst.logList.OnSelected = func(id widget.ListItemID) {
		inst.logList.Unselect(id)
	}

	logContainer := container.NewBorder(
		widget.NewLabel("Installation Log:"), nil, nil, nil,
		inst.logList,
	)

	// Install button
	inst.installBtn = widget.NewButton("Start Installation", func() {
		inst.startInstallation()
	})
	inst.installBtn.Importance = widget.HighImportance

	// Main layout
	content := container.NewBorder(
		container.NewVBox(
			title,
			widget.NewSeparator(),
			stepsContainer,
			widget.NewSeparator(),
			progressContainer,
		),
		container.NewVBox(
			widget.NewSeparator(),
			inst.installBtn,
		),
		nil, nil,
		logContainer,
	)

	return container.NewPadded(content)
}

// startInstallation starts the installation process
func (inst *Installer) startInstallation() {
	if inst.installing {
		return
	}
	inst.installing = true
	inst.installBtn.Disable()
	inst.installBtn.SetText("Installing...")
	inst.logs = make([]string, 0)
	inst.logList.Refresh()

	go func() {
		hasError := false

		// Step 1: Install Docker
		if err := inst.installDocker(); err != nil {
			inst.addLog("✗ Error: " + err.Error())
			hasError = true
		}

		// Step 2: Download Worker
		if err := inst.downloadAndExtractWorker(); err != nil {
			inst.addLog("✗ Error: " + err.Error())
			hasError = true
		}

		// Step 3: Setup VPN
		if err := inst.setupVPN(); err != nil {
			inst.addLog("✗ Error: " + err.Error())
			hasError = true
		}

		// Step 4: Setup auto-start
		if err := inst.setupWorkerAutoStart(); err != nil {
			inst.addLog("✗ Error: " + err.Error())
			hasError = true
		}

		// Create desktop shortcut
		inst.createDesktopShortcut()

		inst.setProgress(100, "Complete")
		inst.installing = false

		if hasError {
			inst.installBtn.SetText("Complete (with errors)")
			inst.addLog("⚠ Installation complete with some errors")
		} else {
			inst.installBtn.SetText("Installation Complete!")
			inst.addLog("✓ All steps completed successfully!")
		}
	}()
}

// hideWindow returns SysProcAttr to hide console window
func hideWindow() *syscall.SysProcAttr {
	return &syscall.SysProcAttr{HideWindow: true}
}

// checkDockerInstalled checks if Docker is installed
func checkDockerInstalled() bool {
	cmd := exec.Command("docker", "--version")
	cmd.SysProcAttr = hideWindow()
	err := cmd.Run()
	return err == nil
}

// checkDockerRunning checks if Docker is running
func checkDockerRunning() bool {
	cmd := exec.Command("docker", "info")
	cmd.SysProcAttr = hideWindow()
	err := cmd.Run()
	return err == nil
}

// downloadFileWithProgress downloads file with progress
func (inst *Installer) downloadFileWithProgress(url, destPath string, baseProgress, maxProgress float64) error {
	resp, err := http.Get(url)
	if err != nil {
		return fmt.Errorf("download failed: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("HTTP error: %d", resp.StatusCode)
	}

	out, err := os.Create(destPath)
	if err != nil {
		return fmt.Errorf("failed to create file: %v", err)
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
				progress := baseProgress + (float64(downloaded)/float64(totalSize))*(maxProgress-baseProgress)
				sizeMB := float64(downloaded) / 1024 / 1024
				totalMB := float64(totalSize) / 1024 / 1024
				inst.setProgress(progress, fmt.Sprintf("Downloading... %.1f / %.1f MB", sizeMB, totalMB))
			}
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("read failed: %v", err)
		}
	}

	return nil
}

// installDocker installs Docker Desktop
func (inst *Installer) installDocker() error {
	inst.setStepStatus(1, "running")
	inst.addLog("→ Checking Docker installation...")

	if checkDockerInstalled() {
		inst.addLog("✓ Docker is installed")

		// Set Docker auto-start
		inst.addLog("→ Setting Docker auto-start...")
		inst.setDockerAutoStart()

		if !checkDockerRunning() {
			inst.addLog("⚠ Docker is not running, starting...")
			dockerExe := `C:\Program Files\Docker\Docker\Docker Desktop.exe`
			cmd := exec.Command(dockerExe)
			cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
			cmd.Start()

			for i := 0; i < 30; i++ {
				time.Sleep(2 * time.Second)
				inst.setProgress(float64(5+i), fmt.Sprintf("Waiting for Docker... %ds", (i+1)*2))
				if checkDockerRunning() {
					inst.addLog("✓ Docker started")
					inst.setStepStatus(1, "done")
					return nil
				}
			}
			inst.addLog("⚠ Docker start timeout")
		} else {
			inst.addLog("✓ Docker is running")
		}
		inst.setStepStatus(1, "done")
		return nil
	}

	inst.addLog("→ Downloading Docker Desktop installer...")
	inst.setProgress(5, "Downloading Docker Desktop...")

	tempDir := os.TempDir()
	installerPath := filepath.Join(tempDir, "DockerDesktopInstaller.exe")

	err := inst.downloadFileWithProgress(DOCKER_DOWNLOAD_URL, installerPath, 5, 25)
	if err != nil {
		inst.setStepStatus(1, "error")
		return fmt.Errorf("failed to download Docker installer: %v", err)
	}

	inst.addLog("→ Installing Docker Desktop...")
	inst.setProgress(30, "Installing Docker Desktop...")

	cmd := exec.Command(installerPath, "install", "--quiet", "--accept-license")
	cmd.SysProcAttr = hideWindow()
	err = cmd.Run()
	if err != nil {
		inst.setStepStatus(1, "error")
		return fmt.Errorf("failed to install Docker: %v", err)
	}

	os.Remove(installerPath)

	// Set Docker auto-start
	inst.addLog("→ Setting Docker auto-start...")
	inst.setDockerAutoStart()

	inst.addLog("✓ Docker Desktop installed")
	inst.addLog("⚠ Please restart computer to complete Docker installation")
	inst.setStepStatus(1, "done")

	return nil
}

// setDockerAutoStart sets Docker to auto-start
func (inst *Installer) setDockerAutoStart() {
	// Method 1: Registry
	regPath := `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
	dockerPath := `"C:\Program Files\Docker\Docker\Docker Desktop.exe"`

	cmd := exec.Command("reg", "add", regPath, "/v", "Docker Desktop", "/t", "REG_SZ", "/d", dockerPath, "/f")
	cmd.SysProcAttr = hideWindow()
	if err := cmd.Run(); err != nil {
		// Method 2: Docker settings.json
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

	inst.addLog("✓ Docker auto-start configured")
}

// getUpdateInfo gets latest version info
func getUpdateInfo() (*Artifact, string, error) {
	resp, err := http.Get(UPDATE_SERVER + "/worker/manifest")
	if err != nil {
		return nil, "", fmt.Errorf("failed to get update info: %v", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, "", fmt.Errorf("failed to read response: %v", err)
	}

	var manifest UpdateManifest
	if err := json.Unmarshal(body, &manifest); err != nil {
		return nil, "", fmt.Errorf("failed to parse manifest: %v", err)
	}

	if manifest.Latest == "" {
		return nil, "", fmt.Errorf("no latest version found")
	}

	versionInfo, ok := manifest.Versions[manifest.Latest]
	if !ok {
		return nil, "", fmt.Errorf("version %s info not found", manifest.Latest)
	}

	osName := runtime.GOOS
	archName := runtime.GOARCH

	for _, artifact := range versionInfo.Artifacts {
		if artifact.OS == osName && artifact.Arch == archName {
			return &artifact, manifest.Latest, nil
		}
	}

	return nil, "", fmt.Errorf("no package found for %s/%s", osName, archName)
}

// downloadAndExtractWorker downloads and extracts Worker
func (inst *Installer) downloadAndExtractWorker() error {
	inst.setStepStatus(2, "running")
	inst.addLog("→ Checking latest version...")

	artifact, version, err := getUpdateInfo()
	if err != nil {
		inst.setStepStatus(2, "error")
		return err
	}

	inst.addLog(fmt.Sprintf("→ Latest version: %s", version))
	inst.addLog(fmt.Sprintf("→ File size: %.2f MB", float64(artifact.Size)/1024/1024))

	if err := os.MkdirAll(INSTALL_DIR, 0755); err != nil {
		inst.setStepStatus(2, "error")
		return fmt.Errorf("failed to create install directory: %v", err)
	}

	tempFile := filepath.Join(os.TempDir(), artifact.Filename)
	inst.addLog("→ Downloading Worker...")

	err = inst.downloadFileWithProgress(artifact.DownloadURL, tempFile, 35, 55)
	if err != nil {
		inst.setStepStatus(2, "error")
		return err
	}

	if artifact.SHA256 != "" {
		inst.addLog("→ Verifying file integrity...")
		fileData, err := os.ReadFile(tempFile)
		if err != nil {
			inst.setStepStatus(2, "error")
			return fmt.Errorf("failed to read file: %v", err)
		}

		hash := sha256.Sum256(fileData)
		hashStr := hex.EncodeToString(hash[:])

		if hashStr != artifact.SHA256 {
			os.Remove(tempFile)
			inst.setStepStatus(2, "error")
			return fmt.Errorf("SHA256 verification failed")
		}
		inst.addLog("✓ SHA256 verified")
	}

	inst.addLog("→ Extracting...")
	inst.setProgress(60, "Extracting...")

	if strings.HasSuffix(artifact.Filename, ".zip") {
		err = unzip(tempFile, INSTALL_DIR)
	} else {
		destPath := filepath.Join(INSTALL_DIR, artifact.Filename)
		err = copyFile(tempFile, destPath)
	}

	if err != nil {
		inst.setStepStatus(2, "error")
		return fmt.Errorf("extraction failed: %v", err)
	}

	os.Remove(tempFile)

	inst.addLog(fmt.Sprintf("✓ Worker installed to %s", INSTALL_DIR))
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

// setupVPN sets up VPN connection
func (inst *Installer) setupVPN() error {
	inst.setStepStatus(3, "running")
	inst.addLog("→ Setting up VPN...")

	vpnDir := filepath.Join(INSTALL_DIR, "vpn")
	if err := os.MkdirAll(vpnDir, 0755); err != nil {
		inst.setStepStatus(3, "error")
		return fmt.Errorf("failed to create VPN directory: %v", err)
	}

	configPath := filepath.Join(vpnDir, "wg0.conf")
	vpnExePath := filepath.Join(vpnDir, "hivemind-vpn.exe")

	// Copy VPN files
	srcVpn := filepath.Join(INSTALL_DIR, "hivemind-vpn.exe")
	if _, err := os.Stat(srcVpn); err == nil {
		copyFile(srcVpn, vpnExePath)
	}

	wintunSrc := filepath.Join(INSTALL_DIR, "wintun.dll")
	wintunDst := filepath.Join(vpnDir, "wintun.dll")
	if _, err := os.Stat(wintunSrc); err == nil {
		copyFile(wintunSrc, wintunDst)
	}

	inst.addLog("→ Requesting VPN configuration...")
	inst.setProgress(70, "Requesting VPN config...")

	err := requestVPNConfig(configPath)
	if err != nil {
		inst.addLog(fmt.Sprintf("⚠ Failed to get VPN config: %v", err))
		inst.setStepStatus(3, "warning")
		return nil
	}

	inst.addLog("✓ VPN configuration received")

	// Check VPN executable
	if _, err := os.Stat(vpnExePath); os.IsNotExist(err) {
		inst.addLog("⚠ VPN executable not found")
		inst.setStepStatus(3, "warning")
		return nil
	}

	inst.addLog("→ Connecting to VPN...")
	inst.setProgress(75, "Connecting VPN...")

	cmd := exec.Command(vpnExePath, configPath, "--auto-retry")
	cmd.Dir = vpnDir
	err = cmd.Start()
	if err != nil {
		inst.addLog(fmt.Sprintf("⚠ Failed to start VPN: %v", err))
		inst.setStepStatus(3, "warning")
		return nil
	}

	time.Sleep(5 * time.Second)

	if pingTest("10.0.0.1") {
		inst.addLog("✓ VPN connected successfully")
		inst.installVPNAutoStart(vpnExePath, configPath)
		inst.setStepStatus(3, "done")
	} else {
		inst.addLog("⚠ VPN connection test failed")
		inst.setStepStatus(3, "warning")
	}

	return nil
}

// requestVPNConfig requests VPN configuration
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
		errMsg := "unknown error"
		if e, ok := result["error"].(string); ok {
			errMsg = e
		}
		return fmt.Errorf("API error: %s", errMsg)
	}

	config, ok := result["config"].(string)
	if !ok || config == "" {
		return fmt.Errorf("config is empty")
	}

	return os.WriteFile(configPath, []byte(config), 0600)
}

// pingTest tests ping
func pingTest(ip string) bool {
	cmd := exec.Command("ping", "-n", "3", "-w", "1000", ip)
	cmd.SysProcAttr = hideWindow()
	output, err := cmd.CombinedOutput()
	if err != nil {
		return false
	}
	return strings.Contains(string(output), "TTL=") || strings.Contains(string(output), "ttl=")
}

// installVPNAutoStart installs VPN auto-start
func (inst *Installer) installVPNAutoStart(vpnExePath, configPath string) {
	inst.addLog("→ Setting VPN auto-start...")

	taskName := "HiveMind-VPN-AutoStart"
	delCmd := exec.Command("schtasks", "/delete", "/tn", taskName, "/f")
	delCmd.SysProcAttr = hideWindow()
	delCmd.Run()

	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s" "%s" --auto-retry`, vpnExePath, configPath),
		"/sc", "onstart",
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)
	cmd.SysProcAttr = hideWindow()

	if err := cmd.Run(); err != nil {
		inst.addLog("⚠ Failed to set VPN auto-start")
	} else {
		inst.addLog("✓ VPN auto-start configured")
	}
}

// setupWorkerAutoStart sets up Worker auto-start
func (inst *Installer) setupWorkerAutoStart() error {
	inst.setStepStatus(4, "running")
	inst.addLog("→ Setting Worker auto-start...")
	inst.setProgress(85, "Configuring auto-start...")

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
		inst.addLog("⚠ Worker executable not found")
		inst.setStepStatus(4, "warning")
		return nil
	}

	taskName := "HiveMind-Worker-AutoStart"
	delCmd := exec.Command("schtasks", "/delete", "/tn", taskName, "/f")
	delCmd.SysProcAttr = hideWindow()
	delCmd.Run()

	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s"`, workerExe),
		"/sc", "onstart",
		"/delay", "0001:00",
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)
	cmd.SysProcAttr = hideWindow()

	if err := cmd.Run(); err != nil {
		inst.addLog("⚠ Failed to set Worker auto-start")
		inst.setStepStatus(4, "warning")
		return nil
	}

	inst.addLog("✓ Worker auto-start configured")
	inst.addLog(fmt.Sprintf("→ Worker location: %s", workerExe))
	inst.setStepStatus(4, "done")

	return nil
}

// createDesktopShortcut creates desktop shortcut
func (inst *Installer) createDesktopShortcut() {
	inst.addLog("→ Creating desktop shortcut...")
	inst.setProgress(95, "Creating shortcut...")

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

	cmd := exec.Command("powershell", "-WindowStyle", "Hidden", "-Command", script)
	cmd.SysProcAttr = hideWindow()
	if err := cmd.Run(); err != nil {
		inst.addLog("⚠ Failed to create desktop shortcut")
	} else {
		inst.addLog("✓ Desktop shortcut created")
	}
}

// Run runs the installer
func (inst *Installer) Run() {
	inst.window.SetContent(inst.buildUI())
	inst.addLog("Ready. Click 'Start Installation' to begin.")
	inst.window.ShowAndRun()
}

func main() {
	// 檢查管理員權限
	if !isAdmin() {
		runAsAdmin()
		os.Exit(0)
	}

	installer := NewInstaller()
	installer.Run()
}

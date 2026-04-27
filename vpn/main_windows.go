//go:build windows

/* SPDX-License-Identifier: MIT
 *
 * HiveMind WireGuard VPN for Windows
 */

package main

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun"
)

var Version = "1.0.0"

const (
	VPN_API_URL     = "https://hivemind.justin0711.com/api/vpn/join"
	GATEWAY_IP      = "10.0.0.1"
	MAX_RETRY_COUNT = 3
	PING_TIMEOUT    = 5 * time.Second
	// 連線後健康檢查
	HEALTH_CHECK_INTERVAL = 10 * time.Second
	HEALTH_FAIL_THRESHOLD = 3 // 連續失敗次數達到才判定斷線
)

const (
	// 若 config 沒有設定 PersistentKeepalive，可用環境變數注入預設值（秒）
	// 建議 25 秒用於穿越 NAT；設為 0 或空字串表示不注入。
	ENV_DEFAULT_KEEPALIVE = "HIVEMIND_WG_DEFAULT_KEEPALIVE"
)

func printUsage() {
	fmt.Printf("HiveMind WireGuard VPN v%s\n", Version)
	fmt.Printf("Usage: %s <config-file> [options]\n", os.Args[0])
	fmt.Println("\nOptions:")
	fmt.Println("  --auto-retry    自動請求新配置並重試連接")
	fmt.Println("  --install       安裝開機自動啟動 (需要管理員權限)")
	fmt.Println("  --uninstall     移除開機自動啟動")
	fmt.Println("  --version       顯示版本")
	fmt.Println("\nExample:")
	fmt.Println("  hivemind-vpn.exe wg0.conf")
	fmt.Println("  hivemind-vpn.exe wg0.conf --auto-retry")
	fmt.Println("  hivemind-vpn.exe wg0.conf --install      # 安裝為開機自啟動")
}

// pingTest 測試 VPN 連接是否正常
func pingTest(ip string, timeout time.Duration) bool {
	log.Printf("正在測試連接 %s ...", ip)

	// 使用 Windows ping 命令
	cmd := exec.Command("ping", "-n", "3", "-w", "1000", ip)
	output, err := cmd.CombinedOutput()

	if err != nil {
		log.Printf("Ping 失敗: %v", err)
		return false
	}

	// 檢查輸出是否包含成功的回應
	outputStr := string(output)
	if strings.Contains(outputStr, "TTL=") || strings.Contains(outputStr, "ttl=") {
		log.Printf("Ping %s 成功!", ip)
		return true
	}

	log.Printf("Ping %s 無回應", ip)
	return false
}

// requestNewConfig 從 API 請求新的 VPN 配置
func requestNewConfig(configPath string) error {
	log.Println("正在請求新的 VPN 配置...")

	hostname, _ := os.Hostname()
	clientName := fmt.Sprintf("worker-%s-%d", hostname, os.Getpid())

	// 準備請求
	reqBody, _ := json.Marshal(map[string]string{
		"client_name": clientName,
	})

	client := &http.Client{Timeout: 15 * time.Second}
	resp, err := client.Post(VPN_API_URL, "application/json", bytes.NewBuffer(reqBody))
	if err != nil {
		return fmt.Errorf("請求失敗: %v", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("讀取回應失敗: %v", err)
	}

	// 記錄 HTTP 狀態碼和回應內容以便除錯
	log.Printf("API 回應狀態碼: %d", resp.StatusCode)
	if resp.StatusCode != http.StatusOK {
		log.Printf("API 回應內容: %s", string(body))
		return fmt.Errorf("API 返回錯誤狀態碼 %d: %s", resp.StatusCode, string(body))
	}

	var result map[string]interface{}
	if err := json.Unmarshal(body, &result); err != nil {
		log.Printf("無法解析的回應內容: %s", string(body))
		return fmt.Errorf("解析回應失敗: %v", err)
	}

	if success, ok := result["success"].(bool); !ok || !success {
		errMsg := "未知錯誤"
		if e, ok := result["error"].(string); ok {
			errMsg = e
		}
		return fmt.Errorf("API 返回錯誤: %s", errMsg)
	}

	config, ok := result["config"].(string)
	if !ok || config == "" {
		return fmt.Errorf("API 返回的配置為空")
	}

	// 寫入配置文件
	if err := os.WriteFile(configPath, []byte(config), 0600); err != nil {
		return fmt.Errorf("寫入配置文件失敗: %v", err)
	}

	log.Printf("新配置已保存到 %s", configPath)
	return nil
}

// installAutoStart 安裝開機自動啟動 (使用任務排程器)
func installAutoStart(configPath string) error {
	exePath, err := os.Executable()
	if err != nil {
		return fmt.Errorf("無法獲取執行文件路徑: %v", err)
	}
	exePath, _ = filepath.Abs(exePath)
	configPath, _ = filepath.Abs(configPath)
	exeDir := filepath.Dir(exePath)

	// 使用 PowerShell 建立排程任務
	taskName := "HiveMind-VPN-AutoStart"

	// 先嘗試刪除舊任務
	exec.Command("schtasks", "/delete", "/tn", taskName, "/f").Run()

	// 建立新任務 - 開機時以 SYSTEM 權限執行
	cmd := exec.Command("schtasks", "/create",
		"/tn", taskName,
		"/tr", fmt.Sprintf(`"%s" "%s" --auto-retry`, exePath, configPath),
		"/sc", "onstart",
		"/ru", "SYSTEM",
		"/rl", "highest",
		"/f",
	)
	cmd.Dir = exeDir
	output, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("建立排程任務失敗: %v\n輸出: %s", err, string(output))
	}

	fmt.Println("========================================")
	fmt.Println("  ✓ 已安裝開機自動啟動!")
	fmt.Printf("  任務名稱: %s\n", taskName)
	fmt.Printf("  執行程式: %s\n", exePath)
	fmt.Printf("  配置文件: %s\n", configPath)
	fmt.Println("========================================")
	fmt.Println("\n提示: 使用 --uninstall 可以移除開機自動啟動")

	return nil
}

// uninstallAutoStart 移除開機自動啟動
func uninstallAutoStart() error {
	taskName := "HiveMind-VPN-AutoStart"

	cmd := exec.Command("schtasks", "/delete", "/tn", taskName, "/f")
	output, err := cmd.CombinedOutput()
	if err != nil {
		// 檢查是否是因為任務不存在
		if strings.Contains(string(output), "不存在") || strings.Contains(string(output), "does not exist") {
			fmt.Println("開機自動啟動任務不存在")
			return nil
		}
		return fmt.Errorf("刪除排程任務失敗: %v\n輸出: %s", err, string(output))
	}

	fmt.Println("========================================")
	fmt.Println("  ✓ 已移除開機自動啟動!")
	fmt.Println("========================================")

	return nil
}

func loadWintunDLLForMain() error {
	// 獲取當前執行文件所在目錄
	exePath, err := os.Executable()
	if err != nil {
		return fmt.Errorf("無法獲取執行文件路徑: %v", err)
	}
	exeDir := filepath.Dir(exePath)

	// 嘗試從多個可能的位置加載 wintun.dll
	possiblePaths := []string{
		filepath.Join(exeDir, "wintun.dll"),
		"wintun.dll",
		filepath.Join(os.Getenv("PROGRAMFILES"), "WireGuard", "wintun.dll"),
		filepath.Join(os.Getenv("SYSTEMROOT"), "System32", "wintun.dll"),
	}

	for _, path := range possiblePaths {
		if _, err := os.Stat(path); err == nil {
			_, err := windows.LoadLibrary(path)
			if err == nil {
				log.Printf("成功加載 wintun.dll: %s", path)
				return nil
			}
		}
	}

	return fmt.Errorf("無法找到或加載 wintun.dll")
}

func parseConfigAddress(configPath string) (localIP, subnetMask string, err error) {
	file, err := os.Open(configPath)
	if err != nil {
		return "", "", err
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if strings.HasPrefix(strings.ToLower(line), "address") {
			parts := strings.SplitN(line, "=", 2)
			if len(parts) == 2 {
				addrStr := strings.TrimSpace(parts[1])
				// 處理 CIDR 格式 (如 10.0.0.2/24)
				if strings.Contains(addrStr, "/") {
					ip, ipNet, err := net.ParseCIDR(addrStr)
					if err != nil {
						return "", "", fmt.Errorf("無效的地址格式: %v", err)
					}
					mask := ipNet.Mask
					return ip.String(), fmt.Sprintf("%d.%d.%d.%d", mask[0], mask[1], mask[2], mask[3]), nil
				}
				return addrStr, "255.255.255.0", nil
			}
		}
	}
	return "", "", fmt.Errorf("配置文件中找不到 Address 設定")
}

// resolveEndpoint 預先解析 endpoint 的 DNS，返回 IP:port 格式
func resolveEndpoint(endpoint string) string {
	// 分離 host 和 port
	host, port, err := net.SplitHostPort(endpoint)
	if err != nil {
		log.Printf("警告: 無法解析 endpoint 格式 %s: %v", endpoint, err)
		return endpoint
	}

	// 檢查是否已經是 IP 地址
	if ip := net.ParseIP(host); ip != nil {
		return endpoint // 已經是 IP，直接返回
	}

	// 解析 DNS
	log.Printf("正在解析 DNS: %s", host)
	ips, err := net.LookupIP(host)
	if err != nil {
		log.Printf("警告: DNS 解析失敗 %s: %v，將使用原始值", host, err)
		return endpoint
	}

	// 優先使用 IPv4
	for _, ip := range ips {
		if ipv4 := ip.To4(); ipv4 != nil {
			resolved := net.JoinHostPort(ipv4.String(), port)
			log.Printf("DNS 解析成功: %s -> %s", endpoint, resolved)
			return resolved
		}
	}

	// 如果沒有 IPv4，使用第一個 IP
	if len(ips) > 0 {
		resolved := net.JoinHostPort(ips[0].String(), port)
		log.Printf("DNS 解析成功: %s -> %s", endpoint, resolved)
		return resolved
	}

	return endpoint
}

func configToUAPI(configPath string) (string, error) {
	file, err := os.Open(configPath)
	if err != nil {
		return "", err
	}
	defer file.Close()

	var uapiLines []string
	scanner := bufio.NewScanner(file)
	inPeer := false
	seenKeepalive := false

	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())

		// 跳過空行和註釋
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		// 處理 section
		if strings.ToLower(line) == "[interface]" {
			inPeer = false
			continue
		}
		if strings.ToLower(line) == "[peer]" {
			inPeer = true
			uapiLines = append(uapiLines, "public_key=")
			continue
		}

		// 處理鍵值對
		parts := strings.SplitN(line, "=", 2)
		if len(parts) != 2 {
			continue
		}

		key := strings.ToLower(strings.TrimSpace(parts[0]))
		value := strings.TrimSpace(parts[1])

		switch key {
		case "privatekey":
			decoded, err := base64.StdEncoding.DecodeString(value)
			if err != nil {
				return "", fmt.Errorf("無效的私鑰: %v", err)
			}
			uapiLines = append(uapiLines, "private_key="+hex.EncodeToString(decoded))

		case "publickey":
			decoded, err := base64.StdEncoding.DecodeString(value)
			if err != nil {
				return "", fmt.Errorf("無效的公鑰: %v", err)
			}
			// 更新最後的 public_key= 行
			for i := len(uapiLines) - 1; i >= 0; i-- {
				if uapiLines[i] == "public_key=" {
					uapiLines[i] = "public_key=" + hex.EncodeToString(decoded)
					break
				}
			}

		case "endpoint":
			if inPeer {
				// 預先解析 DNS，避免 TUN 創建後 DNS 查詢失敗
				resolvedEndpoint := resolveEndpoint(value)
				uapiLines = append(uapiLines, "endpoint="+resolvedEndpoint)
			}

		case "allowedips":
			if inPeer {
				ips := strings.Split(value, ",")
				for _, ip := range ips {
					uapiLines = append(uapiLines, "allowed_ip="+strings.TrimSpace(ip))
				}
			}

		case "persistentkeepalive":
			if inPeer {
				uapiLines = append(uapiLines, "persistent_keepalive_interval="+value)
				seenKeepalive = true
			}

		case "listenport":
			if !inPeer {
				uapiLines = append(uapiLines, "listen_port="+value)
			}

		case "presharedkey":
			if inPeer {
				decoded, err := base64.StdEncoding.DecodeString(value)
				if err == nil {
					uapiLines = append(uapiLines, "preshared_key="+hex.EncodeToString(decoded))
				}
			}
		}
	}

	// 若 config 沒有 keepalive，允許用環境變數注入預設值（改善 NAT/閒置斷線）
	if inPeer && !seenKeepalive {
		if v := strings.TrimSpace(os.Getenv(ENV_DEFAULT_KEEPALIVE)); v != "" && v != "0" {
			uapiLines = append(uapiLines, "persistent_keepalive_interval="+v)
			log.Printf("未在設定檔中找到 PersistentKeepalive，已注入預設 keepalive=%ss (%s)", v, ENV_DEFAULT_KEEPALIVE)
		}
	}

	return strings.Join(uapiLines, "\n"), nil
}

func monitorConnection(wgDevice *device.Device, configPath string, autoRetry bool) {
	// 連線後持續監控。如果連續 ping 失敗，視為連線不健康並嘗試重連。
	failCount := 0
	ticker := time.NewTicker(HEALTH_CHECK_INTERVAL)
	defer ticker.Stop()

	for range ticker.C {
		ok := pingTest(GATEWAY_IP, PING_TIMEOUT)
		if ok {
			if failCount != 0 {
				log.Printf("連線恢復：ping 成功（先前連續失敗 %d 次）", failCount)
			}
			failCount = 0
			continue
		}

		failCount++
		log.Printf("連線健康檢查失敗：連續 %d/%d 次 ping 失敗", failCount, HEALTH_FAIL_THRESHOLD)
		if failCount < HEALTH_FAIL_THRESHOLD {
			continue
		}

		log.Printf("判定連線不健康（連續 %d 次 ping 失敗），準備重連...", failCount)
		// 先關掉現有裝置，避免殘留介面
		wgDevice.Close()

		// auto-retry 模式下先嘗試拉新配置，避免配置過期/被回收
		if autoRetry {
			if err := requestNewConfig(configPath); err != nil {
				log.Printf("警告: 重連前請求新配置失敗: %v（將使用既有配置嘗試重連）", err)
			}
		}

		// 用 connectVPN 重新建立裝置
		newWG, newTUN, connected := connectVPN(configPath)
		if !connected {
			log.Printf("重連失敗：將於下一輪健康檢查再試")
			failCount = 0
			continue
		}

		// 成功後接手，再繼續監控新裝置
		log.Printf("重連成功")
		defer newTUN.Close()
		wgDevice = newWG
		failCount = 0
	}
}

// connectVPN 嘗試連接 VPN，返回設備和連接狀態
func connectVPN(configPath string) (*device.Device, tun.Device, bool) {
	// 解析 IP 地址
	localIP, subnetMask, err := parseConfigAddress(configPath)
	if err != nil {
		log.Printf("解析配置失敗: %v", err)
		return nil, nil, false
	}
	log.Printf("本地 IP: %s/%s", localIP, subnetMask)

	// 創建 TUN 設備
	interfaceName := "HiveMind"
	tunDevice, err := tun.CreateTUN(interfaceName, 0)
	if err != nil {
		log.Printf("創建 TUN 設備失敗: %v", err)
		return nil, nil, false
	}

	realName, err := tunDevice.Name()
	if err == nil {
		log.Printf("TUN 設備名稱: %s", realName)
	}

	// 創建 WireGuard 設備
	logger := device.NewLogger(device.LogLevelVerbose, "wireguard: ")
	bind := conn.NewDefaultBind()
	wgDevice := device.NewDevice(tunDevice, bind, logger)

	// 轉換配置為 UAPI 格式
	uapiConfig, err := configToUAPI(configPath)
	if err != nil {
		log.Printf("轉換配置失敗: %v", err)
		tunDevice.Close()
		wgDevice.Close()
		return nil, nil, false
	}

	// 應用配置
	if err := wgDevice.IpcSet(uapiConfig); err != nil {
		log.Printf("應用配置失敗: %v", err)
		tunDevice.Close()
		wgDevice.Close()
		return nil, nil, false
	}

	// 啟動設備
	if err := wgDevice.Up(); err != nil {
		log.Printf("啟動設備失敗: %v", err)
		tunDevice.Close()
		wgDevice.Close()
		return nil, nil, false
	}

	// 設定 IP 地址
	cmd := exec.Command("netsh", "interface", "ip", "set", "address",
		fmt.Sprintf("name=%s", interfaceName),
		"static", localIP, subnetMask)
	output, err := cmd.CombinedOutput()
	if err != nil {
		log.Printf("警告: 設定 IP 地址失敗: %v\n輸出: %s", err, string(output))
	} else {
		log.Printf("IP 地址設定成功: %s/%s", localIP, subnetMask)
	}

	// 等待網路介面就緒
	time.Sleep(2 * time.Second)

	// Ping 測試
	if pingTest(GATEWAY_IP, PING_TIMEOUT) {
		return wgDevice, tunDevice, true
	}

	// Ping 失敗，關閉連接
	log.Println("VPN 連接測試失敗，關閉連接...")
	wgDevice.Close()
	tunDevice.Close()
	return nil, nil, false
}

func main() {
	if len(os.Args) >= 2 && os.Args[1] == "--version" {
		fmt.Printf("HiveMind WireGuard VPN v%s\n", Version)
		return
	}

	// 處理 --uninstall (不需要配置文件)
	for _, arg := range os.Args[1:] {
		if arg == "--uninstall" {
			if err := uninstallAutoStart(); err != nil {
				log.Fatalf("移除失敗: %v", err)
			}
			return
		}
	}

	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	configPath := os.Args[1]

	// 解析選項
	autoRetry := false
	install := false
	for _, arg := range os.Args[2:] {
		switch arg {
		case "--auto-retry":
			autoRetry = true
		case "--install":
			install = true
		}
	}

	// 處理 --install
	if install {
		if err := installAutoStart(configPath); err != nil {
			log.Fatalf("安裝失敗: %v", err)
		}
		return
	}

	// 載入 wintun.dll
	if err := loadWintunDLLForMain(); err != nil {
		log.Fatalf("載入 wintun.dll 失敗: %v", err)
	}

	var wgDevice *device.Device
	var tunDevice tun.Device
	var connected bool

	for retry := 0; retry <= MAX_RETRY_COUNT; retry++ {
		// 檢查配置文件是否存在，如果不存在且啟用自動重試，則請求新配置
		if _, err := os.Stat(configPath); os.IsNotExist(err) {
			if autoRetry {
				log.Printf("配置文件不存在，正在請求新配置...")
				if err := requestNewConfig(configPath); err != nil {
					log.Printf("請求新配置失敗: %v", err)
					if retry < MAX_RETRY_COUNT {
						log.Printf("等待 5 秒後重試... (%d/%d)", retry+1, MAX_RETRY_COUNT)
						time.Sleep(5 * time.Second)
						continue
					}
					log.Fatalf("無法獲取 VPN 配置")
				}
			} else {
				log.Fatalf("配置文件不存在: %s", configPath)
			}
		}

		log.Printf("嘗試連接 VPN... (第 %d 次)", retry+1)
		wgDevice, tunDevice, connected = connectVPN(configPath)

		if connected {
			break
		}

		// 連接失敗
		if autoRetry && retry < MAX_RETRY_COUNT {
			log.Println("連接失敗，正在請求新的 VPN 配置...")
			if err := requestNewConfig(configPath); err != nil {
				log.Printf("請求新配置失敗: %v", err)
			}
			log.Printf("等待 3 秒後重試... (%d/%d)", retry+1, MAX_RETRY_COUNT)
			time.Sleep(3 * time.Second)
		} else if !autoRetry {
			log.Println("連接失敗。使用 --auto-retry 參數可自動請求新配置並重試")
			os.Exit(1)
		}
	}

	if !connected {
		log.Fatalf("經過 %d 次重試後仍無法連接 VPN", MAX_RETRY_COUNT)
	}

	defer wgDevice.Close()
	defer tunDevice.Close()

	fmt.Println("========================================")
	fmt.Println("  ✓ HiveMind VPN 已連接!")
	fmt.Println("  ✓ Ping 測試通過")
	fmt.Println("  按 Ctrl+C 斷開連接")
	fmt.Println("========================================")

	// 等待中斷信號
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)

	// 背景監控連線健康狀態（避免「連一段時間就斷」）
	go monitorConnection(wgDevice, configPath, autoRetry)
	<-sigChan

	fmt.Println("\n正在斷開 VPN 連接...")
}

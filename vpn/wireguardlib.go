package main

import (
	"C"
	"bufio"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"log"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"

	"golang.org/x/sys/windows"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun"
)

// 全局變量用於跟蹤活動的連接
var (
	activeDevices    = make(map[string]*device.Device)
	activeTunDevices = make(map[string]tun.Device)
	activeMutex      sync.Mutex
	wintunDLLLoaded  bool
	wintunDLLHandle  uintptr
)

// 加載 wintun.dll 的函數
func loadWintunDLL() error {
	if wintunDLLLoaded {
		return nil
	}

	// 獲取當前執行文件所在目錄
	exePath, err := os.Executable()
	if err != nil {
		return fmt.Errorf("無法獲取執行文件路徑: %v", err)
	}
	exeDir := filepath.Dir(exePath)

	// 嘗試從多個可能的位置加載 wintun.dll
	possiblePaths := []string{
		filepath.Join(exeDir, "wintun.dll"), // 執行文件所在目錄
		"wintun.dll",                        // 當前工作目錄
		filepath.Join(os.Getenv("PROGRAMFILES"), "WireGuard", "wintun.dll"),
		filepath.Join(os.Getenv("PROGRAMFILES(X86)"), "WireGuard", "wintun.dll"),
		filepath.Join(os.Getenv("SYSTEMROOT"), "System32", "wintun.dll"),
		filepath.Join(os.Getenv("SYSTEMROOT"), "SysWOW64", "wintun.dll"),
	}

	for _, path := range possiblePaths {
		if _, err := os.Stat(path); err == nil {
			// 使用 LoadLibrary 加載 DLL
			handle, err := windows.LoadLibrary(path)
			if err == nil {
				wintunDLLHandle = uintptr(handle)
				wintunDLLLoaded = true
				log.Printf("成功加載 wintun.dll: %s", path)
				return nil
			} else {
				log.Printf("嘗試加載 %s 失敗: %v", path, err)
			}
		}
	}

	return fmt.Errorf("無法找到或加載 wintun.dll。請確保 wintun.dll 位於可執行文件同一目錄或系統路徑中")
}

//export StartWireGuard
func StartWireGuard(configPath *C.char) *C.char {
	cfgPath := C.GoString(configPath)

	// 確保 wintun.dll 已加載
	if err := loadWintunDLL(); err != nil {
		return C.CString(fmt.Sprintf("ERROR: %v", err))
	}

	localIP, subnetMask, err := parseInterfaceAddress(cfgPath)
	if err != nil {
		return C.CString(fmt.Sprintf("ERROR: Failed to read local IP from config: %v", err))
	}

	interfaceName := "myvpn0"
	tunDevice, err := tun.CreateTUN(interfaceName, 0)
	if err != nil {
		return C.CString(fmt.Sprintf("ERROR: Failed to create TUN device: %v", err))
	}

	logger := device.NewLogger(device.LogLevelVerbose, "wireguard: ")
	bind := conn.NewDefaultBind()
	wgDevice := device.NewDevice(tunDevice, bind, logger)

	uapiConfig, err := fileToUAPI(cfgPath)
	if err != nil {
		tunDevice.Close()
		return C.CString(fmt.Sprintf("ERROR: Failed to convert config to UAPI format: %v", err))
	}

	err = wgDevice.IpcSet(uapiConfig)
	if err != nil {
		tunDevice.Close()
		wgDevice.Close()
		return C.CString(fmt.Sprintf("ERROR: Failed to apply configuration via IPC: %v", err))
	}

	err = wgDevice.Up()
	if err != nil {
		tunDevice.Close()
		wgDevice.Close()
		return C.CString(fmt.Sprintf("ERROR: Failed to bring up device: %v", err))
	}

	cmd := exec.Command("netsh", "interface", "ip", "set", "address",
		fmt.Sprintf("name=\"%s\"", interfaceName),
		"static",
		localIP,
		subnetMask,
	)

	output, err := cmd.CombinedOutput()
	if err != nil {
		tunDevice.Close()
		wgDevice.Close()
		return C.CString(fmt.Sprintf("ERROR: Failed to set IP address for interface '%s'. Error: %v\nOutput: %s",
			interfaceName, err, string(output)))
	}

	// 存儲活動設備
	activeMutex.Lock()
	activeDevices[cfgPath] = wgDevice
	activeTunDevices[cfgPath] = tunDevice
	activeMutex.Unlock()

	return C.CString("SUCCESS: WireGuard connection established")
}

//export StopWireGuard
func StopWireGuard(configPath *C.char) *C.char {
	cfgPath := C.GoString(configPath)

	activeMutex.Lock()
	defer activeMutex.Unlock()

	if wgDevice, exists := activeDevices[cfgPath]; exists {
		wgDevice.Close()
		delete(activeDevices, cfgPath)
	}

	if tunDevice, exists := activeTunDevices[cfgPath]; exists {
		tunDevice.Close()
		delete(activeTunDevices, cfgPath)

		// 清理 IP 地址
		interfaceName := "myvpn0"
		localIP, _, err := parseInterfaceAddress(cfgPath)
		if err == nil {
			cleanupCmd := exec.Command("netsh", "interface", "ip", "delete", "address",
				fmt.Sprintf("name=\"%s\"", interfaceName),
				fmt.Sprintf("addr=%s", localIP),
			)
			cleanupCmd.Run()
		}
	}

	return C.CString("SUCCESS: WireGuard connection stopped")
}

//export GetStatus
func GetStatus(configPath *C.char) *C.char {
	cfgPath := C.GoString(configPath)

	activeMutex.Lock()
	defer activeMutex.Unlock()

	if _, exists := activeDevices[cfgPath]; exists {
		return C.CString("CONNECTED")
	}
	return C.CString("DISCONNECTED")
}

func parseInterfaceAddress(filePath string) (ip, subnetMask string, err error) {
	file, err := os.Open(filePath)
	if err != nil {
		return "", "", err
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if strings.HasPrefix(line, "Address") {
			parts := strings.SplitN(line, "=", 2)
			if len(parts) != 2 {
				continue
			}
			addressCIDR := strings.TrimSpace(parts[1])
			ipAddr, ipNet, err := net.ParseCIDR(addressCIDR)
			if err != nil {
				return "", "", fmt.Errorf("invalid CIDR address: %w", err)
			}
			mask := fmt.Sprintf("%d.%d.%d.%d", ipNet.Mask[0], ipNet.Mask[1], ipNet.Mask[2], ipNet.Mask[3])
			return ipAddr.String(), mask, nil
		}
	}
	return "", "", fmt.Errorf("Address not found in [Interface] section of config file")
}

func fileToUAPI(filePath string) (string, error) {
	file, err := os.Open(filePath)
	if err != nil {
		return "", err
	}
	defer file.Close()

	var uapi strings.Builder
	scanner := bufio.NewScanner(file)
	inPeerSection := false

	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		if strings.HasPrefix(line, "[Interface]") {
			inPeerSection = false
			continue
		} else if strings.HasPrefix(line, "[Peer]") {
			inPeerSection = true
			continue
		}

		if strings.HasPrefix(line, "Address") {
			continue
		}

		parts := strings.SplitN(line, "=", 2)
		if len(parts) != 2 {
			continue
		}
		key := strings.TrimSpace(parts[0])
		value := strings.TrimSpace(parts[1])

		key = strings.ToLower(key)

		if !inPeerSection {
			switch key {
			case "privatekey":
				keyBytes, err := base64.StdEncoding.DecodeString(value)
				if err != nil {
					return "", fmt.Errorf("invalid base64 private key: %w", err)
				}
				fmt.Fprintf(&uapi, "private_key=%s\n", hex.EncodeToString(keyBytes))
			case "listenport":
				fmt.Fprintf(&uapi, "listen_port=%s\n", value)
			}
		} else {
			switch key {
			case "publickey":
				keyBytes, err := base64.StdEncoding.DecodeString(value)
				if err != nil {
					return "", fmt.Errorf("invalid base64 public key: %w", err)
				}
				fmt.Fprintf(&uapi, "public_key=%s\n", hex.EncodeToString(keyBytes))
			case "allowedips":
				for _, ip := range strings.Split(value, ",") {
					fmt.Fprintf(&uapi, "allowed_ip=%s\n", strings.TrimSpace(ip))
				}
			case "endpoint":
				addr, err := net.ResolveUDPAddr("udp", value)
				if err == nil {
					fmt.Fprintf(&uapi, "endpoint=%s\n", addr.String())
				} else {
					fmt.Fprintf(&uapi, "endpoint=%s\n", value)
				}
			}
		}
	}

	if err := scanner.Err(); err != nil {
		return "", err
	}
	return uapi.String(), nil
}

// 需要一個空的main函數來編譯為C共享庫
func main() {}

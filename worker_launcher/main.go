package main

import (
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"
)

func main() {
	workerDir, err := resolveWorkerDir()
	if err != nil {
		fatal(err)
	}

	workerExe := filepath.Join(workerDir, "worker_node.exe")
	if _, err := os.Stat(workerExe); err != nil {
		fatal(fmt.Errorf("worker executable not found: %s", workerExe))
	}

	vpnCmd, vpnStarted := tryStartVPN(workerDir)
	waitForVPN(60 * time.Second)

	workerCmd, err := startWorker(workerDir, workerExe)
	if err != nil {
		stopProcess(vpnCmd)
		fatal(err)
	}

	// Ctrl+C / close：確保把 VPN 一起停掉
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, os.Interrupt, syscall.SIGTERM)

	workerDone := make(chan error, 1)
	go func() { workerDone <- workerCmd.Wait() }()

	select {
	case sig := <-sigCh:
		fmt.Printf("[launcher] received %v, stopping...\n", sig)
		stopProcess(workerCmd)
		stopProcess(vpnCmd)
		_ = vpnStarted
		os.Exit(0)
	case err := <-workerDone:
		// worker 結束時一併關 VPN
		stopProcess(vpnCmd)
		_ = vpnStarted
		if err != nil {
			fatal(err)
		}
	}
}

func resolveWorkerDir() (string, error) {
	self, err := os.Executable()
	if err != nil {
		return "", err
	}
	self, err = filepath.Abs(self)
	if err != nil {
		return "", err
	}
	return filepath.Dir(self), nil
}

func tryStartVPN(workerDir string) (*exec.Cmd, bool) {
	vpnExe := filepath.Join(workerDir, "hivemind-vpn.exe")
	wgConf := filepath.Join(workerDir, "wg0.conf")
	if _, err := os.Stat(vpnExe); err != nil {
		fmt.Printf("[launcher] vpn executable not found, skip: %s\n", vpnExe)
		return nil, false
	}
	if _, err := os.Stat(wgConf); err != nil {
		fmt.Printf("[launcher] wg0.conf not found, skip: %s\n", wgConf)
		return nil, false
	}

	cmd := exec.Command(vpnExe, "wg0.conf", "--auto-retry")
	cmd.Dir = workerDir
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	fmt.Printf("[launcher] starting vpn: %s\n", vpnExe)
	if err := cmd.Start(); err != nil {
		fmt.Printf("[launcher] vpn start failed: %v\n", err)
		return nil, false
	}
	return cmd, true
}

func waitForVPN(timeout time.Duration) {
	deadline := time.Now().Add(timeout)
	for {
		if hasVPNIP() {
			fmt.Printf("[launcher] vpn connected (10.0.0.x detected)\n")
			return
		}
		if time.Now().After(deadline) {
			fmt.Printf("[launcher] vpn not detected after %s, continue to start worker\n", timeout)
			return
		}
		time.Sleep(2 * time.Second)
	}
}

func hasVPNIP() bool {
	ifaces, err := net.Interfaces()
	if err != nil {
		return false
	}
	for _, iface := range ifaces {
		// Skip down interfaces.
		if iface.Flags&net.FlagUp == 0 {
			continue
		}
		addrs, err := iface.Addrs()
		if err != nil {
			continue
		}
		for _, addr := range addrs {
			ip := ipFromAddr(addr)
			if ip == nil {
				continue
			}
			if isVPNSubnet(ip) {
				return true
			}
		}
	}
	return false
}

func ipFromAddr(a net.Addr) net.IP {
	switch v := a.(type) {
	case *net.IPNet:
		return v.IP
	case *net.IPAddr:
		return v.IP
	default:
		return nil
	}
}

func isVPNSubnet(ip net.IP) bool {
	ip4 := ip.To4()
	if ip4 == nil {
		return false
	}
	return ip4[0] == 10 && ip4[1] == 0 && ip4[2] == 0
}

func startWorker(workerDir, workerExe string) (*exec.Cmd, error) {
	cmd := exec.Command(workerExe)
	cmd.Dir = workerDir
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin

	fmt.Printf("[launcher] starting worker: %s\n", workerExe)
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	return cmd, nil
}

func stopProcess(cmd *exec.Cmd) {
	if cmd == nil || cmd.Process == nil {
		return
	}
	_ = cmd.Process.Kill()
}

func ensureFileCopy(src, dst string) {
	if _, err := os.Stat(dst); err == nil {
		return
	}
	if _, err := os.Stat(src); err != nil {
		return
	}
	_ = os.MkdirAll(filepath.Dir(dst), 0o755)
	_ = copyFile(src, dst)
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer func() { _ = out.Close() }()
	if _, err := out.ReadFrom(in); err != nil {
		return err
	}
	return out.Close()
}

func fatal(err error) {
	if err == nil {
		return
	}
	if errors.Is(err, os.ErrNotExist) {
		fmt.Fprintf(os.Stderr, "[launcher] %v\n", err)
		os.Exit(2)
	}
	fmt.Fprintf(os.Stderr, "[launcher] %v\n", err)
	os.Exit(1)
}

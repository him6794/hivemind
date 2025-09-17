// Package main provides system monitoring capabilities as a replacement for Python psutil
// This implementation includes C exports for Python ctypes integration
package main

import "C"
import (
	"fmt"
	"io/ioutil"
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unsafe"
)

// MemoryInfo represents memory usage information
type MemoryInfo struct {
	Total     uint64  `json:"total"`
	Available uint64  `json:"available"`
	Used      uint64  `json:"used"`
	Free      uint64  `json:"free"`
	Percent   float64 `json:"percent"`
}

// GPUInfo represents GPU information
type GPUInfo struct {
	Name      string  `json:"name"`
	Memory    float64 `json:"memory_gb"`
	Score     int     `json:"score"`
	Available bool    `json:"available"`
}

// Monitor provides system monitoring capabilities
type Monitor struct {
	lastCPUTimes map[string]uint64
	lastSample   time.Time
}

// NewMonitor creates a new monitor instance
func NewMonitor() *Monitor {
	return &Monitor{
		lastCPUTimes: make(map[string]uint64),
		lastSample:   time.Now(),
	}
}

// Global monitor instance for C exports
var globalMonitor = NewMonitor()

// GetCPUCount returns the number of CPU cores
func (m *Monitor) GetCPUCount() int {
	switch runtime.GOOS {
	case "windows":
		return m.getCPUCountWindows()
	default:
		return runtime.NumCPU()
	}
}

func (m *Monitor) getCPUCountWindows() int {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	getSystemInfo := kernel32.NewProc("GetSystemInfo")

	type systemInfo struct {
		wProcessorArchitecture      uint16
		wReserved                   uint16
		dwPageSize                  uint32
		lpMinimumApplicationAddress uintptr
		lpMaximumApplicationAddress uintptr
		dwActiveProcessorMask       uintptr
		dwNumberOfProcessors        uint32
		dwProcessorType             uint32
		dwAllocationGranularity     uint32
		wProcessorLevel             uint16
		wProcessorRevision          uint16
	}

	var si systemInfo
	getSystemInfo.Call(uintptr(unsafe.Pointer(&si)))
	return int(si.dwNumberOfProcessors)
}

// GetMemoryInfo returns memory usage information
func (m *Monitor) GetMemoryInfo() (*MemoryInfo, error) {
	switch runtime.GOOS {
	case "windows":
		return m.getMemoryInfoWindows()
	case "linux":
		return m.getMemoryInfoLinux()
	default:
		return nil, fmt.Errorf("unsupported platform: %s", runtime.GOOS)
	}
}

func (m *Monitor) getMemoryInfoWindows() (*MemoryInfo, error) {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	globalMemoryStatusEx := kernel32.NewProc("GlobalMemoryStatusEx")

	type memoryStatusEx struct {
		dwLength                uint32
		dwMemoryLoad            uint32
		ullTotalPhys            uint64
		ullAvailPhys            uint64
		ullTotalPageFile        uint64
		ullAvailPageFile        uint64
		ullTotalVirtual         uint64
		ullAvailVirtual         uint64
		ullAvailExtendedVirtual uint64
	}

	var memStatus memoryStatusEx
	memStatus.dwLength = uint32(unsafe.Sizeof(memStatus))

	ret, _, _ := globalMemoryStatusEx.Call(uintptr(unsafe.Pointer(&memStatus)))
	if ret == 0 {
		return nil, fmt.Errorf("failed to get memory status")
	}

	used := memStatus.ullTotalPhys - memStatus.ullAvailPhys
	percent := float64(used) / float64(memStatus.ullTotalPhys) * 100

	return &MemoryInfo{
		Total:     memStatus.ullTotalPhys,
		Available: memStatus.ullAvailPhys,
		Used:      used,
		Free:      memStatus.ullAvailPhys,
		Percent:   percent,
	}, nil
}

func (m *Monitor) getMemoryInfoLinux() (*MemoryInfo, error) {
	data, err := ioutil.ReadFile("/proc/meminfo")
	if err != nil {
		return nil, err
	}

	lines := strings.Split(string(data), "\n")
	memInfo := make(map[string]uint64)

	for _, line := range lines {
		fields := strings.Fields(line)
		if len(fields) >= 2 {
			key := strings.TrimSuffix(fields[0], ":")
			value, _ := strconv.ParseUint(fields[1], 10, 64)
			memInfo[key] = value * 1024 // Convert from kB to bytes
		}
	}

	total := memInfo["MemTotal"]
	available := memInfo["MemAvailable"]
	if available == 0 {
		available = memInfo["MemFree"] + memInfo["Buffers"] + memInfo["Cached"]
	}
	used := total - available
	percent := float64(used) / float64(total) * 100

	return &MemoryInfo{
		Total:     total,
		Available: available,
		Used:      used,
		Free:      memInfo["MemFree"],
		Percent:   percent,
	}, nil
}

// GetHostname returns the system hostname
func (m *Monitor) GetHostname() (string, error) {
	return os.Hostname()
}

// BenchmarkCPU performs a simple CPU benchmark
func (m *Monitor) BenchmarkCPU() int {
	start := time.Now()
	iterations := 1000000
	result := 0

	for i := 0; i < iterations; i++ {
		result += i % 1000
	}

	duration := time.Since(start)
	score := int(float64(iterations) / duration.Seconds() / 1000)
	if score < 1 {
		score = 1
	}
	return score
}

// GetGPUInfo detects GPU information
func (m *Monitor) GetGPUInfo() (*GPUInfo, error) {
	switch runtime.GOOS {
	case "windows":
		return m.getGPUInfoWindows()
	default:
		return &GPUInfo{"Not Detected", 0.0, 0, false}, nil
	}
}

func (m *Monitor) getGPUInfoWindows() (*GPUInfo, error) {
	// Try PowerShell approach for better reliability
	cmd := exec.Command("powershell", "-Command",
		"Get-WmiObject -Class Win32_VideoController | Select-Object -First 1 Name, AdapterRAM | ConvertTo-Csv -NoTypeInformation")
	output, err := cmd.Output()
	if err == nil {
		lines := strings.Split(string(output), "\n")
		if len(lines) >= 2 {
			// Parse CSV output
			fields := strings.Split(strings.Trim(lines[1], "\r"), ",")
			if len(fields) >= 2 {
				name := strings.Trim(fields[0], "\"")
				memStr := strings.Trim(fields[1], "\"")

				memory := uint64(0)
				if memStr != "" && memStr != "null" {
					if mem, err := strconv.ParseUint(memStr, 10, 64); err == nil {
						memory = mem
					}
				}

				memoryGB := float64(memory) / (1024 * 1024 * 1024)
				score := int(memoryGB * 1000)
				if score < 1000 && memory > 0 {
					score = 1000
				}

				return &GPUInfo{name, memoryGB, score, memory > 0}, nil
			}
		}
	}

	return &GPUInfo{"Not Detected", 0.0, 0, false}, nil
}

// C export functions for Python ctypes integration

//export get_cpu_count
func get_cpu_count() C.int {
	return C.int(globalMonitor.GetCPUCount())
}

//export get_memory_total
func get_memory_total() C.ulonglong {
	mem, err := globalMonitor.GetMemoryInfo()
	if err != nil {
		return 0
	}
	return C.ulonglong(mem.Total)
}

//export get_memory_available
func get_memory_available() C.ulonglong {
	mem, err := globalMonitor.GetMemoryInfo()
	if err != nil {
		return 0
	}
	return C.ulonglong(mem.Available)
}

//export get_memory_percent
func get_memory_percent() C.double {
	mem, err := globalMonitor.GetMemoryInfo()
	if err != nil {
		return 0.0
	}
	return C.double(mem.Percent)
}

//export get_hostname_length
func get_hostname_length() C.int {
	hostname, err := globalMonitor.GetHostname()
	if err != nil {
		return 0
	}
	return C.int(len(hostname))
}

//export get_hostname_data
func get_hostname_data(buf *C.char, size C.int) C.int {
	hostname, err := globalMonitor.GetHostname()
	if err != nil {
		return 0
	}

	hostnameBytes := []byte(hostname)
	maxLen := int(size) - 1
	if len(hostnameBytes) > maxLen {
		hostnameBytes = hostnameBytes[:maxLen]
	}

	// Copy to C buffer
	for i, b := range hostnameBytes {
		*(*C.char)(unsafe.Pointer(uintptr(unsafe.Pointer(buf)) + uintptr(i))) = C.char(b)
	}
	// Null terminate
	*(*C.char)(unsafe.Pointer(uintptr(unsafe.Pointer(buf)) + uintptr(len(hostnameBytes)))) = 0

	return C.int(len(hostnameBytes))
}

//export get_cpu_score
func get_cpu_score() C.int {
	return C.int(globalMonitor.BenchmarkCPU())
}

//export get_gpu_name_length
func get_gpu_name_length() C.int {
	gpu, err := globalMonitor.GetGPUInfo()
	if err != nil {
		return 0
	}
	return C.int(len(gpu.Name))
}

//export get_gpu_name_data
func get_gpu_name_data(buf *C.char, size C.int) C.int {
	gpu, err := globalMonitor.GetGPUInfo()
	if err != nil {
		return 0
	}

	nameBytes := []byte(gpu.Name)
	maxLen := int(size) - 1
	if len(nameBytes) > maxLen {
		nameBytes = nameBytes[:maxLen]
	}

	// Copy to C buffer
	for i, b := range nameBytes {
		*(*C.char)(unsafe.Pointer(uintptr(unsafe.Pointer(buf)) + uintptr(i))) = C.char(b)
	}
	// Null terminate
	*(*C.char)(unsafe.Pointer(uintptr(unsafe.Pointer(buf)) + uintptr(len(nameBytes)))) = 0

	return C.int(len(nameBytes))
}

//export get_gpu_memory
func get_gpu_memory() C.double {
	gpu, err := globalMonitor.GetGPUInfo()
	if err != nil {
		return 0.0
	}
	return C.double(gpu.Memory)
}

//export get_gpu_score
func get_gpu_score() C.int {
	gpu, err := globalMonitor.GetGPUInfo()
	if err != nil {
		return 0
	}
	return C.int(gpu.Score)
}

//export get_gpu_available
func get_gpu_available() C.int {
	gpu, err := globalMonitor.GetGPUInfo()
	if err != nil {
		return 0
	}
	if gpu.Available {
		return 1
	}
	return 0
}

func main() {
	// Test the monitor functionality
	monitor := NewMonitor()

	fmt.Println("=== Go System Monitor ===")
	fmt.Printf("CPU Cores: %d\n", monitor.GetCPUCount())

	if mem, err := monitor.GetMemoryInfo(); err == nil {
		fmt.Printf("Memory: %.1fGB total, %.1f%% used\n",
			float64(mem.Total)/(1024*1024*1024), mem.Percent)
	}

	if hostname, err := monitor.GetHostname(); err == nil {
		fmt.Printf("Hostname: %s\n", hostname)
	}

	fmt.Printf("CPU Score: %d\n", monitor.BenchmarkCPU())

	if gpu, err := monitor.GetGPUInfo(); err == nil {
		fmt.Printf("GPU: %s, %.1fGB, Score: %d\n", gpu.Name, gpu.Memory, gpu.Score)
	}
}

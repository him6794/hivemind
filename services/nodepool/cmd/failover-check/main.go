package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

type apiClient struct {
	base string
	http *http.Client
}

type workerInfo struct {
	ID     string            `json:"id"`
	Addr   string            `json:"addr"`
	Status string            `json:"status"`
	Meta   map[string]string `json:"meta"`
}

type registerWorkerPayload struct {
	Username    string `json:"username"`
	IP          string `json:"ip"`
	CPUCores    int    `json:"cpu_cores"`
	MemoryGB    int    `json:"memory_gb"`
	CPUScore    int    `json:"cpu_score"`
	GPUScore    int    `json:"gpu_score"`
	GPUMemoryGB int    `json:"gpu_memory_gb"`
	Location    string `json:"location"`
}

func newAPIClient(base string) *apiClient {
	return &apiClient{
		base: strings.TrimRight(base, "/"),
		http: &http.Client{Timeout: 10 * time.Second},
	}
}

func (c *apiClient) requestJSON(method, path string, payload any, token string) (int, map[string]any, error) {
	var body io.Reader
	if payload != nil {
		raw, err := json.Marshal(payload)
		if err != nil {
			return 0, nil, err
		}
		body = bytes.NewReader(raw)
	}
	req, err := http.NewRequest(method, c.base+path, body)
	if err != nil {
		return 0, nil, err
	}
	if payload != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if strings.TrimSpace(token) != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return resp.StatusCode, nil, err
	}
	out := map[string]any{}
	if len(bytes.TrimSpace(raw)) > 0 {
		if err := json.Unmarshal(raw, &out); err != nil {
			out["raw"] = string(raw)
		}
	}
	return resp.StatusCode, out, nil
}

func intFromMeta(meta map[string]string, key string, fallback int) int {
	if meta == nil {
		return fallback
	}
	raw := strings.TrimSpace(meta[key])
	if raw == "" {
		return fallback
	}
	value, err := strconv.Atoi(raw)
	if err != nil {
		return fallback
	}
	return value
}

func workerRegisterPayload(worker workerInfo) registerWorkerPayload {
	location := "Local"
	if worker.Meta != nil && strings.TrimSpace(worker.Meta["location"]) != "" {
		location = worker.Meta["location"]
	}
	return registerWorkerPayload{
		Username:    worker.ID,
		IP:          worker.Addr,
		CPUCores:    intFromMeta(worker.Meta, "cpu_cores", 8),
		MemoryGB:    intFromMeta(worker.Meta, "memory_gb", 16),
		CPUScore:    intFromMeta(worker.Meta, "cpu_score", 100),
		GPUScore:    intFromMeta(worker.Meta, "gpu_score", 80),
		GPUMemoryGB: intFromMeta(worker.Meta, "gpu_memory", 8),
		Location:    location,
	}
}

func findTask(tasks []map[string]any, taskID string) map[string]any {
	for _, task := range tasks {
		if fmt.Sprint(task["TaskID"]) == taskID || fmt.Sprint(task["task_id"]) == taskID {
			return task
		}
	}
	return nil
}

func boolField(data map[string]any, key string) bool {
	v, _ := data[key].(bool)
	return v
}

func stringField(data map[string]any, key string) string {
	if data == nil {
		return ""
	}
	if v, ok := data[key].(string); ok {
		return v
	}
	return ""
}

func login(client *apiClient, username, password string) (string, error) {
	var last map[string]any
	for i := 0; i < 20; i++ {
		status, data, err := client.requestJSON(http.MethodPost, "/api/login", map[string]string{
			"username": username,
			"password": password,
		}, "")
		if err == nil && status == http.StatusOK && boolField(data, "success") && stringField(data, "token") != "" {
			return stringField(data, "token"), nil
		}
		last = data
		time.Sleep(time.Second)
	}
	return "", fmt.Errorf("login failed for %s: %v", username, last)
}

func ensureWorkerUsers(client *apiClient) {
	for _, username := range []string{"worker1", "worker2", "worker3"} {
		_, _, _ = client.requestJSON(http.MethodPost, "/api/register", map[string]string{
			"username": username,
			"password": "worker123",
		}, "")
	}
}

func getWorkers(client *apiClient, token string) ([]workerInfo, error) {
	status, data, err := client.requestJSON(http.MethodGet, "/api/workers?include_offline=true", nil, token)
	if err != nil {
		return nil, err
	}
	if status != http.StatusOK {
		return nil, fmt.Errorf("get workers returned HTTP %d: %v", status, data)
	}
	rawWorkers, ok := data["workers"].([]any)
	if !ok {
		return nil, fmt.Errorf("workers response missing workers: %v", data)
	}
	workers := make([]workerInfo, 0, len(rawWorkers))
	for _, raw := range rawWorkers {
		blob, _ := json.Marshal(raw)
		var worker workerInfo
		if err := json.Unmarshal(blob, &worker); err != nil {
			return nil, err
		}
		workers = append(workers, worker)
	}
	return workers, nil
}

func activeWorkers(workers []workerInfo) []workerInfo {
	active := make([]workerInfo, 0, len(workers))
	for _, worker := range workers {
		if worker.Status == "ACTIVE" {
			active = append(active, worker)
		}
	}
	return active
}

func registerWorker(client *apiClient, token string, worker workerInfo) (map[string]any, error) {
	status, data, err := client.requestJSON(http.MethodPost, "/api/register-worker", workerRegisterPayload(worker), token)
	if err != nil {
		return nil, err
	}
	if status != http.StatusOK {
		return nil, fmt.Errorf("register worker returned HTTP %d: %v", status, data)
	}
	return data, nil
}

func submitTask(client *apiClient, token, taskID, torrent string) (map[string]any, error) {
	status, data, err := client.requestJSON(http.MethodPost, "/api/upload-task", map[string]any{
		"task_id":       taskID,
		"torrent":       torrent,
		"memory_gb":     1,
		"gpu_memory_gb": 0,
		"host_count":    1,
	}, token)
	if err != nil {
		return nil, err
	}
	if status != http.StatusOK {
		return nil, fmt.Errorf("upload task returned HTTP %d: %v", status, data)
	}
	return data, nil
}

func getTask(client *apiClient, token, taskID string) (map[string]any, error) {
	values := url.Values{"limit": {"50"}, "offset": {"0"}, "q": {taskID}}
	status, data, err := client.requestJSON(http.MethodGet, "/api/tasks?"+values.Encode(), nil, token)
	if err != nil {
		return nil, err
	}
	if status != http.StatusOK || !boolField(data, "success") {
		return nil, fmt.Errorf("list tasks failed HTTP %d: %v", status, data)
	}
	rawTasks, ok := data["tasks"].([]any)
	if !ok {
		return nil, errors.New("tasks response missing tasks")
	}
	tasks := make([]map[string]any, 0, len(rawTasks))
	for _, raw := range rawTasks {
		if task, ok := raw.(map[string]any); ok {
			tasks = append(tasks, task)
		}
	}
	return findTask(tasks, taskID), nil
}

func tcpOpen(rawBase string) bool {
	parsed, err := url.Parse(rawBase)
	if err != nil {
		return false
	}
	host := parsed.Host
	if !strings.Contains(host, ":") {
		if parsed.Scheme == "https" {
			host += ":443"
		} else {
			host += ":80"
		}
	}
	conn, err := net.DialTimeout("tcp", host, 1500*time.Millisecond)
	if err != nil {
		return false
	}
	_ = conn.Close()
	return true
}

func printJSON(value any) {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	_ = enc.Encode(value)
}

func runSingle(client *apiClient, token string) error {
	taskID := fmt.Sprintf("single-smoke-%d", time.Now().Unix())
	upload, err := submitTask(client, token, taskID, "magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&dn=single")
	if err != nil {
		return err
	}
	time.Sleep(2 * time.Second)
	task, err := getTask(client, token, taskID)
	if err != nil {
		return err
	}
	if task == nil {
		return errors.New("single mode cannot find submitted task")
	}
	printJSON(map[string]any{
		"mode":           "single",
		"task_id":        taskID,
		"upload":         upload,
		"status":         task["Status"],
		"status_message": task["StatusMessage"],
		"worker_id":      task["WorkerID"],
		"worker_ip":      task["WorkerIP"],
	})
	return nil
}

func runFailover(client *apiClient, adminToken, userToken, adminUser, adminPass, badAddr string, timeout time.Duration) error {
	workers, err := getWorkers(client, adminToken)
	if err != nil {
		return err
	}
	active := activeWorkers(workers)
	if len(active) < 2 {
		return fmt.Errorf("need >=2 ACTIVE workers, got %d", len(active))
	}
	victim := active[0]
	for _, worker := range active {
		if worker.ID == adminUser {
			victim = worker
			break
		}
	}
	oldAddr := victim.Addr
	victim.Addr = badAddr
	registerToken := adminToken
	if victim.ID != adminUser {
		var err error
		registerToken, err = login(client, victim.ID, adminPass)
		if err != nil {
			return err
		}
	}
	if _, err := registerWorker(client, registerToken, victim); err != nil {
		return err
	}

	step1 := fmt.Sprintf("failover-step1-%d", time.Now().Unix())
	if _, err := submitTask(client, userToken, step1, "magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&dn=step1"); err != nil {
		return err
	}

	deadline := time.Now().Add(timeout)
	probeRound := 0
	lastProbe := time.Time{}
	offline := false
	for time.Now().Before(deadline) {
		workers, err := getWorkers(client, adminToken)
		if err != nil {
			return err
		}
		for _, worker := range workers {
			if worker.ID == victim.ID && worker.Status == "OFFLINE" {
				offline = true
				break
			}
		}
		if offline {
			break
		}
		if time.Since(lastProbe) >= 2*time.Second {
			probeRound++
			taskID := fmt.Sprintf("failover-probe-%d-%d", time.Now().Unix(), probeRound)
			if _, err := submitTask(client, userToken, taskID, "magnet:?xt=urn:btih:cccccccccccccccccccccccccccccccccccccccc&dn=probe"); err != nil {
				return err
			}
			lastProbe = time.Now()
		}
		time.Sleep(time.Second)
	}
	if !offline {
		return fmt.Errorf("victim %s did not become OFFLINE within %s", victim.ID, timeout)
	}

	step2 := fmt.Sprintf("failover-step2-%d", time.Now().Unix())
	upload2, err := submitTask(client, userToken, step2, "magnet:?xt=urn:btih:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb&dn=step2")
	if err != nil {
		return err
	}
	time.Sleep(2 * time.Second)
	task2, err := getTask(client, userToken, step2)
	if err != nil {
		return err
	}
	msg := stringField(task2, "StatusMessage")
	if strings.Contains(msg, badAddr) {
		return fmt.Errorf("step2 still targeted bad address: %s", msg)
	}
	printJSON(map[string]any{
		"mode":                 "failover",
		"victim":               victim.ID,
		"old_addr":             oldAddr,
		"bad_addr":             badAddr,
		"offline_observed":     offline,
		"step2_upload":         upload2,
		"step2_status":         task2["Status"],
		"step2_status_message": msg,
	})
	return nil
}

func main() {
	base := flag.String("base", "http://127.0.0.1:18082", "master API base")
	adminUser := flag.String("admin-user", "worker1", "admin worker username")
	adminPass := flag.String("admin-pass", "worker123", "admin worker password")
	taskUser := flag.String("task-user", "testuser", "task user")
	taskPass := flag.String("task-pass", "testpass123", "task password")
	badAddr := flag.String("bad-addr", "127.0.0.1:59999", "unreachable worker address to inject")
	timeoutSec := flag.Int("timeout-sec", 30, "wait timeout for OFFLINE transition")
	mode := flag.String("mode", "failover", "single or failover")
	flag.Parse()

	client := newAPIClient(*base)
	if !tcpOpen(*base) {
		fmt.Fprintf(os.Stderr, "[fail] master API is down at %s\n", *base)
		os.Exit(1)
	}
	ensureWorkerUsers(client)

	adminToken, err := login(client, *adminUser, *adminPass)
	if err != nil {
		fmt.Fprintf(os.Stderr, "[fail] %v\n", err)
		os.Exit(1)
	}
	userToken, err := login(client, *taskUser, *taskPass)
	if err != nil {
		fmt.Fprintf(os.Stderr, "[fail] %v\n", err)
		os.Exit(1)
	}

	var runErr error
	switch strings.ToLower(strings.TrimSpace(*mode)) {
	case "single":
		runErr = runSingle(client, userToken)
	case "failover":
		runErr = runFailover(client, adminToken, userToken, *adminUser, *adminPass, *badAddr, time.Duration(*timeoutSec)*time.Second)
	default:
		runErr = fmt.Errorf("unsupported mode: %s", *mode)
	}
	if runErr != nil {
		fmt.Fprintf(os.Stderr, "[fail] %v\n", runErr)
		os.Exit(1)
	}
	fmt.Println("[ok] check passed")
}

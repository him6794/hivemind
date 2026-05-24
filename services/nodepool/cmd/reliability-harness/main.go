package main

import (
	"bytes"
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math/rand"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"hivemind/services/nodepool/internal/netproxy"
)

type harnessArgs struct {
	RepoRoot               string `json:"repo_root"`
	Runs                   int    `json:"runs"`
	FailureSimulations     int    `json:"failure_simulations"`
	LongSeconds            int    `json:"long_seconds"`
	RetrySeconds           int    `json:"retry_seconds"`
	CPUSeconds             int    `json:"cpu_seconds"`
	IOSeconds              int    `json:"io_seconds"`
	FailureSeconds         int    `json:"failure_seconds"`
	ParallelSeconds        int    `json:"parallel_seconds"`
	ParallelTasks          int    `json:"parallel_tasks"`
	LatencyMS              int    `json:"latency_ms"`
	JitterMS               int    `json:"jitter_ms"`
	WorkerNodepoolAddr     string `json:"worker_nodepool_addr"`
	TaskTimeoutSeconds     int    `json:"task_timeout_seconds"`
	TaskWaitTimeoutSeconds int    `json:"task_wait_timeout_seconds"`
	KillMinSeconds         int    `json:"kill_min_seconds"`
	KillMaxSeconds         int    `json:"kill_max_seconds"`
	ReconnectMinSeconds    int    `json:"reconnect_min_seconds"`
	ReconnectMaxSeconds    int    `json:"reconnect_max_seconds"`
	Seed                   int64  `json:"seed"`
	StopOnFailure          bool   `json:"stop_on_failure"`
	Calibration            bool   `json:"calibration"`
}

type summary struct {
	StartedAt    string       `json:"started_at"`
	FinishedAt   string       `json:"finished_at,omitempty"`
	Seed         int64        `json:"seed"`
	Parameters   harnessArgs  `json:"parameters"`
	Runs         []runResult  `json:"runs"`
	Regressions  []regression `json:"regressions"`
	Passed       bool         `json:"passed"`
	DODSatisfied bool         `json:"dod_satisfied"`
	Artifacts    string       `json:"artifacts"`
}

type regression struct {
	Run      int      `json:"run"`
	Failures []string `json:"failures"`
}

type runResult struct {
	Run           int                 `json:"run"`
	RunID         string              `json:"run_id"`
	StartedAt     string              `json:"started_at"`
	FinishedAt    string              `json:"finished_at,omitempty"`
	Tasks         []workloadTask      `json:"tasks"`
	ObservedTasks map[string]taskInfo `json:"observed_tasks"`
	WorkersFinal  []workerInfo        `json:"workers_final"`
	Events        []event             `json:"events"`
	Failures      []string            `json:"failures"`
	Passed        bool                `json:"passed"`
}

type event struct {
	Event    string  `json:"event"`
	Worker   string  `json:"worker,omitempty"`
	PID      int     `json:"pid,omitempty"`
	At       string  `json:"at"`
	DelaySec float64 `json:"delay_sec,omitempty"`
}

type workloadSpec struct {
	Kind      string
	HostCount int
}

type workloadTask struct {
	Kind           string         `json:"kind"`
	TaskID         string         `json:"task_id"`
	BTIH           string         `json:"btih"`
	ExpectedStatus string         `json:"expected_status"`
	Upload         map[string]any `json:"upload"`
}

type taskInfo struct {
	TaskID        string `json:"TaskID"`
	Status        string `json:"Status"`
	StatusMessage string `json:"StatusMessage"`
	ResultTorrent string `json:"ResultTorrent"`
	WorkerID      string `json:"WorkerID"`
	WorkerIP      string `json:"WorkerIP"`
}

type workerInfo struct {
	ID     string            `json:"id"`
	Addr   string            `json:"addr"`
	Status string            `json:"status"`
	Meta   map[string]string `json:"meta"`
}

type managedProcess struct {
	name   string
	cmd    *exec.Cmd
	stdout *os.File
	stderr *os.File
}

type harness struct {
	args      harnessArgs
	root      string
	runRoot   string
	binDir    string
	rng       *rand.Rand
	processes map[string]*managedProcess
	proxies   []*netproxy.DelayProxy
	summary   summary
}

func nowISO() string {
	return time.Now().UTC().Format(time.RFC3339Nano)
}

func exeName(name string) string {
	if strings.EqualFold(os.Getenv("GOOS"), "windows") || strings.EqualFold(filepath.Ext(os.Args[0]), ".exe") {
		if !strings.HasSuffix(name, ".exe") {
			return name + ".exe"
		}
	}
	if os.PathSeparator == '\\' && !strings.HasSuffix(name, ".exe") {
		return name + ".exe"
	}
	return name
}

func defaultRepoRoot() string {
	wd, err := os.Getwd()
	if err != nil {
		return "."
	}
	for dir := wd; ; dir = filepath.Dir(dir) {
		if _, err := os.Stat(filepath.Join(dir, "proto", "hivemind.proto")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return wd
		}
	}
}

func workloadSpecs(args harnessArgs) []workloadSpec {
	specs := []workloadSpec{
		{Kind: "cpu", HostCount: 1},
		{Kind: "io", HostCount: 1},
		{Kind: "failure", HostCount: 1},
		{Kind: "retry", HostCount: 1},
		{Kind: "long", HostCount: 1},
	}
	for i := 0; i < args.ParallelTasks; i++ {
		specs = append(specs, workloadSpec{Kind: fmt.Sprintf("parallel-%d", i), HostCount: 1})
	}
	return specs
}

func dodSatisfied(s summary) bool {
	return s.Passed && s.Parameters.Runs >= 10 && s.Parameters.FailureSimulations >= 3 && s.Parameters.LongSeconds >= 900
}

func newHarness(args harnessArgs) *harness {
	if args.RepoRoot == "" {
		args.RepoRoot = defaultRepoRoot()
	}
	runRoot := filepath.Join(args.RepoRoot, "test_logs", "reliability-go", time.Now().Format("20060102-150405"))
	return &harness{
		args:      args,
		root:      args.RepoRoot,
		runRoot:   runRoot,
		binDir:    filepath.Join(args.RepoRoot, "test_logs", "bin"),
		rng:       rand.New(rand.NewSource(args.Seed)),
		processes: map[string]*managedProcess{},
		summary: summary{
			StartedAt:  nowISO(),
			Seed:       args.Seed,
			Parameters: args,
			Runs:       []runResult{},
			Artifacts:  runRoot,
		},
	}
}

func (h *harness) run() int {
	_ = os.MkdirAll(h.runRoot, 0o755)
	defer h.cleanup()
	if err := h.buildBinaries(); err != nil {
		fmt.Fprintf(os.Stderr, "build failed: %v\n", err)
		return 1
	}
	for i := 1; i <= h.args.Runs; i++ {
		result := h.runOnce(i)
		h.summary.Runs = append(h.summary.Runs, result)
		if !result.Passed {
			h.summary.Regressions = append(h.summary.Regressions, regression{Run: i, Failures: result.Failures})
			if h.args.StopOnFailure {
				break
			}
		}
		h.writeReports()
	}
	h.summary.Passed = len(h.summary.Runs) == h.args.Runs
	for _, run := range h.summary.Runs {
		h.summary.Passed = h.summary.Passed && run.Passed
	}
	h.summary.DODSatisfied = dodSatisfied(h.summary)
	h.summary.FinishedAt = nowISO()
	h.writeReports()
	if h.summary.Passed {
		return 0
	}
	return 1
}

func (h *harness) buildBinaries() error {
	if err := os.MkdirAll(h.binDir, 0o755); err != nil {
		return err
	}
	targets := []struct {
		name string
		cwd  string
		pkg  string
	}{
		{"nodepool", filepath.Join(h.root, "services", "nodepool"), "./cmd/server"},
		{"master", filepath.Join(h.root, "services", "master"), "./cmd/server"},
		{"worker", filepath.Join(h.root, "services", "worker"), "./cmd/server"},
		{"reliability-executor", filepath.Join(h.root, "services", "worker"), "./cmd/reliability-executor"},
	}
	for _, target := range targets {
		out := filepath.Join(h.binDir, exeName(target.name))
		cmd := exec.Command("go", "build", "-o", out, target.pkg)
		cmd.Dir = target.cwd
		raw, err := cmd.CombinedOutput()
		_ = os.WriteFile(filepath.Join(h.runRoot, "build-"+target.name+".log"), raw, 0o644)
		if err != nil {
			return fmt.Errorf("%s: %w: %s", target.name, err, string(raw))
		}
	}
	return nil
}

func (h *harness) runOnce(index int) runResult {
	runID := fmt.Sprintf("rel-go-%s-r%02d", time.Now().Format("20060102150405"), index)
	runDir := filepath.Join(h.runRoot, runID)
	_ = os.MkdirAll(runDir, 0o755)
	result := runResult{
		Run:           index,
		RunID:         runID,
		StartedAt:     nowISO(),
		ObservedTasks: map[string]taskInfo{},
		Events:        []event{},
		Failures:      []string{},
	}
	h.cleanupRuntime()
	defer func() {
		result.FinishedAt = nowISO()
		raw, _ := json.MarshalIndent(result, "", "  ")
		_ = os.WriteFile(filepath.Join(runDir, "run-result.json"), raw, 0o644)
		h.cleanupRuntime()
	}()

	if err := h.startDependencies(runDir); err != nil {
		result.Failures = append(result.Failures, "dependencies: "+err.Error())
		return result
	}
	if err := h.startServices(runDir); err != nil {
		result.Failures = append(result.Failures, "services: "+err.Error())
		return result
	}
	client := newAPIClient("http://127.0.0.1:18082")
	token, err := login(client, "worker1", "worker123")
	if err != nil {
		result.Failures = append(result.Failures, "login: "+err.Error())
		return result
	}
	tasks, err := h.submitWorkload(client, token, runID, runDir)
	if err != nil {
		result.Failures = append(result.Failures, "submit workload: "+err.Error())
		return result
	}
	result.Tasks = tasks
	if ok, err := h.submitDuplicate(client, token, tasks[0].TaskID, runDir); err != nil {
		result.Failures = append(result.Failures, "duplicate submit: "+err.Error())
	} else if ok {
		result.Failures = append(result.Failures, "duplicate submission was accepted")
	}
	h.runFailureSimulations(runDir, &result)
	observed, err := h.waitForTasks(client, token, tasks, runDir)
	if err != nil {
		result.Failures = append(result.Failures, "wait tasks: "+err.Error())
	}
	result.ObservedTasks = observed
	workers, err := getWorkers(client, token)
	if err == nil {
		result.WorkersFinal = workers
	}
	h.evaluateRun(tasks, observed, &result)
	result.Passed = len(result.Failures) == 0
	return result
}

func (h *harness) startDependencies(runDir string) error {
	_ = exec.Command("docker", "rm", "-f", "hivemind-rel-pg", "hivemind-rel-redis").Run()
	if out, err := exec.Command("docker", "run", "-d", "--name", "hivemind-rel-pg", "-e", "POSTGRES_USER=hivemind", "-e", "POSTGRES_PASSWORD=hivemind", "-e", "POSTGRES_DB=hivemind", "-p", "25432:5432", "postgres:16-alpine").CombinedOutput(); err != nil {
		return fmt.Errorf("postgres start: %w: %s", err, string(out))
	}
	if out, err := exec.Command("docker", "run", "-d", "--name", "hivemind-rel-redis", "-p", "26379:6379", "redis:7-alpine").CombinedOutput(); err != nil {
		return fmt.Errorf("redis start: %w: %s", err, string(out))
	}
	deadline := time.Now().Add(60 * time.Second)
	for time.Now().Before(deadline) {
		if exec.Command("docker", "exec", "hivemind-rel-pg", "pg_isready", "-U", "hivemind", "-d", "hivemind").Run() == nil &&
			exec.Command("docker", "exec", "hivemind-rel-redis", "redis-cli", "ping").Run() == nil {
			return nil
		}
		time.Sleep(time.Second)
	}
	return errors.New("docker dependencies did not become ready")
}

func (h *harness) startServices(runDir string) error {
	nodepoolEnv := map[string]string{
		"NODEPOOL_POSTGRES_DSN":             "postgres://hivemind:hivemind@127.0.0.1:25432/hivemind?sslmode=disable",
		"NODEPOOL_REDIS_ADDR":               "127.0.0.1:26379",
		"NODEPOOL_ENABLE_HTTP_AUTH":         "1",
		"NODEPOOL_HTTP_ADDR":                ":18081",
		"NODEPOOL_ADDR":                     ":50051",
		"NODEPOOL_TASK_TIMEOUT_SEC":         fmt.Sprint(h.args.TaskTimeoutSeconds),
		"NODEPOOL_MAX_REDISPATCH":           "3",
		"NODEPOOL_LOG_FILE":                 filepath.Join(runDir, "nodepool-events.log"),
		"NODEPOOL_WORKER_PROBE_TIMEOUT_SEC": "5",
	}
	if err := h.startProcess("nodepool", filepath.Join(h.binDir, exeName("nodepool")), filepath.Join(h.root, "services", "nodepool", "cmd", "server"), runDir, nodepoolEnv); err != nil {
		return err
	}
	if err := waitPort("127.0.0.1", 50051, 30*time.Second); err != nil {
		return err
	}
	if err := waitPort("127.0.0.1", 18081, 30*time.Second); err != nil {
		return err
	}
	h.startProxy("nodepool-grpc", 55051, 50051)
	nodeClient := newAPIClient("http://127.0.0.1:18081")
	for _, user := range []string{"worker1", "worker2", "worker3"} {
		_, _, _ = nodeClient.requestJSON(http.MethodPost, "/api/register", map[string]string{"username": user, "password": "worker123"}, "")
	}

	masterEnv := map[string]string{
		"MASTER_HTTP_ADDR":            ":18082",
		"NODEPOOL_GRPC_ADDR":          "127.0.0.1:55051",
		"NODEPOOL_HTTP_BASE":          "http://127.0.0.1:18081",
		"MASTER_NODEPOOL_TIMEOUT_SEC": "30",
		"BT_PUBLIC_BASE_URL":          "http://127.0.0.1:18082",
	}
	if err := h.startProcess("master", filepath.Join(h.binDir, exeName("master")), filepath.Join(h.root, "services", "master", "cmd", "server"), runDir, masterEnv); err != nil {
		return err
	}
	if err := waitPort("127.0.0.1", 18082, 30*time.Second); err != nil {
		return err
	}

	workerCommon := map[string]string{
		"NODEPOOL_ADDR":                      h.args.WorkerNodepoolAddr,
		"WORKER_PASSWORD":                    "worker123",
		"WORKER_AUTO_REGISTER":               "1",
		"WORKER_REGISTER_TIMEOUT_SEC":        "15",
		"WORKER_REGISTER_RETRY_INTERVAL_SEC": "2",
		"WORKER_EXECUTOR_CMD":                filepath.Join(h.binDir, exeName("reliability-executor")),
		"WORKER_EXECUTOR_TIMEOUT_SEC":        fmt.Sprint(h.args.LongSeconds + 600),
		"WORKER_USAGE_REPORT_INTERVAL_SEC":   "2",
		"RELIABILITY_CPU_SECONDS":            fmt.Sprint(h.args.CPUSeconds),
		"RELIABILITY_IO_SECONDS":             fmt.Sprint(h.args.IOSeconds),
		"RELIABILITY_FAILURE_SECONDS":        fmt.Sprint(h.args.FailureSeconds),
		"RELIABILITY_RETRY_SECONDS":          fmt.Sprint(h.args.RetrySeconds),
		"RELIABILITY_LONG_SECONDS":           fmt.Sprint(h.args.LongSeconds),
		"RELIABILITY_PARALLEL_SECONDS":       fmt.Sprint(h.args.ParallelSeconds),
	}
	for i, ports := range []struct{ grpc, public, control int }{{51053, 50053, 18080}, {51054, 50054, 18083}, {51055, 50055, 18084}} {
		name := fmt.Sprintf("worker%d", i+1)
		env := cloneEnv(workerCommon)
		env["WORKER_USERNAME"] = name
		env["WORKER_ADDR"] = fmt.Sprintf(":%d", ports.grpc)
		env["WORKER_PUBLIC_ADDR"] = fmt.Sprintf("127.0.0.1:%d", ports.public)
		env["WORKER_CONTROL_ADDR"] = fmt.Sprintf(":%d", ports.control)
		if err := h.startProcess(name, filepath.Join(h.binDir, exeName("worker")), filepath.Join(h.root, "services", "worker", "cmd", "server"), runDir, env); err != nil {
			return err
		}
		if err := waitPort("127.0.0.1", ports.grpc, 30*time.Second); err != nil {
			return err
		}
		h.startProxy(name+"-grpc", ports.public, ports.grpc)
	}
	time.Sleep(3 * time.Second)
	return nil
}

func (h *harness) startProxy(name string, listenPort, targetPort int) {
	proxy := netproxy.NewDelayProxy(netproxy.DelayProxyConfig{
		Name:       name,
		ListenHost: "127.0.0.1",
		ListenPort: listenPort,
		TargetHost: "127.0.0.1",
		TargetPort: targetPort,
		Latency:    time.Duration(h.args.LatencyMS) * time.Millisecond,
		Jitter:     time.Duration(h.args.JitterMS) * time.Millisecond,
	})
	if err := proxy.Start(); err == nil {
		h.proxies = append(h.proxies, proxy)
	}
}

func cloneEnv(in map[string]string) map[string]string {
	out := make(map[string]string, len(in))
	for k, v := range in {
		out[k] = v
	}
	return out
}

func (h *harness) startProcess(name, exe, cwd, runDir string, extra map[string]string) error {
	stdout, err := os.Create(filepath.Join(runDir, name+".out.log"))
	if err != nil {
		return err
	}
	stderr, err := os.Create(filepath.Join(runDir, name+".err.log"))
	if err != nil {
		_ = stdout.Close()
		return err
	}
	cmd := exec.Command(exe)
	cmd.Dir = cwd
	cmd.Stdout = stdout
	cmd.Stderr = stderr
	cmd.Env = os.Environ()
	for k, v := range extra {
		cmd.Env = append(cmd.Env, k+"="+v)
	}
	if err := cmd.Start(); err != nil {
		_ = stdout.Close()
		_ = stderr.Close()
		return err
	}
	h.processes[name] = &managedProcess{name: name, cmd: cmd, stdout: stdout, stderr: stderr}
	return nil
}

func waitPort(host string, port int, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	addr := fmt.Sprintf("%s:%d", host, port)
	for time.Now().Before(deadline) {
		conn, err := net.DialTimeout("tcp", addr, 500*time.Millisecond)
		if err == nil {
			_ = conn.Close()
			return nil
		}
		time.Sleep(250 * time.Millisecond)
	}
	return fmt.Errorf("port did not open: %s", addr)
}

type apiClient struct {
	base string
	http *http.Client
}

func newAPIClient(base string) *apiClient {
	return &apiClient{base: strings.TrimRight(base, "/"), http: &http.Client{Timeout: 30 * time.Second}}
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
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	out := map[string]any{}
	if len(bytes.TrimSpace(raw)) > 0 {
		_ = json.Unmarshal(raw, &out)
	}
	return resp.StatusCode, out, nil
}

func login(client *apiClient, username, password string) (string, error) {
	for i := 0; i < 20; i++ {
		status, data, err := client.requestJSON(http.MethodPost, "/api/login", map[string]string{"username": username, "password": password}, "")
		if err == nil && status == http.StatusOK {
			if ok, _ := data["success"].(bool); ok {
				if token, _ := data["token"].(string); token != "" {
					return token, nil
				}
			}
		}
		time.Sleep(time.Second)
	}
	return "", fmt.Errorf("login failed for %s", username)
}

func getWorkers(client *apiClient, token string) ([]workerInfo, error) {
	status, data, err := client.requestJSON(http.MethodGet, "/api/workers?include_offline=true", nil, token)
	if err != nil {
		return nil, err
	}
	if status != http.StatusOK {
		return nil, fmt.Errorf("workers HTTP %d", status)
	}
	raw, _ := json.Marshal(data["workers"])
	var workers []workerInfo
	if err := json.Unmarshal(raw, &workers); err != nil {
		return nil, err
	}
	return workers, nil
}

func (h *harness) submitWorkload(client *apiClient, token, runID, runDir string) ([]workloadTask, error) {
	tasks := make([]workloadTask, 0)
	for _, spec := range workloadSpecs(h.args) {
		taskID := runID + "-" + spec.Kind
		sum := sha1.Sum([]byte(taskID + "|" + spec.Kind))
		btih := hex.EncodeToString(sum[:])
		body := map[string]any{
			"task_id":       taskID,
			"torrent":       "magnet:?xt=urn:btih:" + btih + "&dn=" + url.QueryEscape(spec.Kind),
			"memory_gb":     1,
			"gpu_memory_gb": 0,
			"host_count":    spec.HostCount,
			"max_cpt":       0,
		}
		status, data, err := client.requestJSON(http.MethodPost, "/api/upload-task", body, token)
		if err != nil {
			return nil, err
		}
		if status != http.StatusOK {
			return nil, fmt.Errorf("upload %s HTTP %d: %v", taskID, status, data)
		}
		expected := "COMPLETED"
		if spec.Kind == "failure" {
			expected = "FAILED"
		}
		tasks = append(tasks, workloadTask{Kind: spec.Kind, TaskID: taskID, BTIH: btih, ExpectedStatus: expected, Upload: data})
	}
	raw, _ := json.MarshalIndent(tasks, "", "  ")
	_ = os.WriteFile(filepath.Join(runDir, "submitted-tasks.json"), raw, 0o644)
	return tasks, nil
}

func (h *harness) submitDuplicate(client *apiClient, token, taskID, runDir string) (bool, error) {
	sum := sha1.Sum([]byte(taskID + "|duplicate"))
	btih := hex.EncodeToString(sum[:])
	status, data, err := client.requestJSON(http.MethodPost, "/api/upload-task", map[string]any{
		"task_id":       taskID,
		"torrent":       "magnet:?xt=urn:btih:" + btih + "&dn=duplicate",
		"memory_gb":     1,
		"gpu_memory_gb": 0,
		"host_count":    1,
		"max_cpt":       0,
	}, token)
	raw, _ := json.MarshalIndent(data, "", "  ")
	_ = os.WriteFile(filepath.Join(runDir, "duplicate-response.json"), raw, 0o644)
	if err != nil {
		return false, err
	}
	if status != http.StatusOK {
		return false, fmt.Errorf("duplicate HTTP %d", status)
	}
	ok, _ := data["success"].(bool)
	return ok, nil
}

func (h *harness) runFailureSimulations(runDir string, result *runResult) {
	for i := 0; i < h.args.FailureSimulations; i++ {
		delay := h.randomDelay(h.args.KillMinSeconds, h.args.KillMaxSeconds)
		time.Sleep(delay)
		names := make([]string, 0)
		for name, proc := range h.processes {
			if strings.HasPrefix(name, "worker") && proc.cmd.ProcessState == nil {
				names = append(names, name)
			}
		}
		if len(names) == 0 {
			result.Failures = append(result.Failures, "no live worker available to kill")
			return
		}
		victim := names[h.rng.Intn(len(names))]
		proc := h.processes[victim]
		pid := proc.cmd.Process.Pid
		_ = proc.cmd.Process.Kill()
		result.Events = append(result.Events, event{Event: "worker_killed", Worker: victim, PID: pid, At: nowISO(), DelaySec: delay.Seconds()})
		reconnect := h.randomDelay(h.args.ReconnectMinSeconds, h.args.ReconnectMaxSeconds)
		time.Sleep(reconnect)
		_ = h.restartWorker(victim, runDir)
		result.Events = append(result.Events, event{Event: "worker_restarted", Worker: victim, PID: h.processes[victim].cmd.Process.Pid, At: nowISO(), DelaySec: reconnect.Seconds()})
		time.Sleep(2 * time.Second)
	}
}

func (h *harness) randomDelay(minSec, maxSec int) time.Duration {
	if maxSec < minSec {
		maxSec = minSec
	}
	if minSec < 0 {
		minSec = 0
	}
	if maxSec == minSec {
		return time.Duration(minSec) * time.Second
	}
	return time.Duration(minSec)*time.Second + time.Duration(h.rng.Int63n(int64(maxSec-minSec+1)))*time.Second
}

func (h *harness) restartWorker(name, runDir string) error {
	if old := h.processes[name]; old != nil {
		old.stop()
		delete(h.processes, name)
	}
	idx := int(name[len(name)-1] - '0')
	grpcPort := 51052 + idx
	publicPort := 50052 + idx
	controlPort := map[int]int{1: 18080, 2: 18083, 3: 18084}[idx]
	env := map[string]string{
		"NODEPOOL_ADDR":                      h.args.WorkerNodepoolAddr,
		"WORKER_PASSWORD":                    "worker123",
		"WORKER_AUTO_REGISTER":               "1",
		"WORKER_REGISTER_TIMEOUT_SEC":        "15",
		"WORKER_REGISTER_RETRY_INTERVAL_SEC": "2",
		"WORKER_EXECUTOR_CMD":                filepath.Join(h.binDir, exeName("reliability-executor")),
		"WORKER_EXECUTOR_TIMEOUT_SEC":        fmt.Sprint(h.args.LongSeconds + 600),
		"WORKER_USAGE_REPORT_INTERVAL_SEC":   "2",
		"WORKER_USERNAME":                    name,
		"WORKER_ADDR":                        fmt.Sprintf(":%d", grpcPort),
		"WORKER_PUBLIC_ADDR":                 fmt.Sprintf("127.0.0.1:%d", publicPort),
		"WORKER_CONTROL_ADDR":                fmt.Sprintf(":%d", controlPort),
		"RELIABILITY_CPU_SECONDS":            fmt.Sprint(h.args.CPUSeconds),
		"RELIABILITY_IO_SECONDS":             fmt.Sprint(h.args.IOSeconds),
		"RELIABILITY_FAILURE_SECONDS":        fmt.Sprint(h.args.FailureSeconds),
		"RELIABILITY_RETRY_SECONDS":          fmt.Sprint(h.args.RetrySeconds),
		"RELIABILITY_LONG_SECONDS":           fmt.Sprint(h.args.LongSeconds),
		"RELIABILITY_PARALLEL_SECONDS":       fmt.Sprint(h.args.ParallelSeconds),
	}
	if err := h.startProcess(name, filepath.Join(h.binDir, exeName("worker")), filepath.Join(h.root, "services", "worker", "cmd", "server"), runDir, env); err != nil {
		return err
	}
	return waitPort("127.0.0.1", grpcPort, 30*time.Second)
}

func (p *managedProcess) stop() {
	if p == nil || p.cmd == nil || p.cmd.Process == nil {
		return
	}
	if p.cmd.ProcessState == nil {
		_ = p.cmd.Process.Kill()
		_, _ = p.cmd.Process.Wait()
	}
	_ = p.stdout.Close()
	_ = p.stderr.Close()
}

func (h *harness) waitForTasks(client *apiClient, token string, tasks []workloadTask, runDir string) (map[string]taskInfo, error) {
	deadline := time.Now().Add(time.Duration(h.args.TaskWaitTimeoutSeconds) * time.Second)
	taskIDs := map[string]bool{}
	for _, task := range tasks {
		taskIDs[task.TaskID] = true
	}
	latest := map[string]taskInfo{}
	for time.Now().Before(deadline) {
		status, data, err := client.requestJSON(http.MethodGet, "/api/tasks?limit=200&sort_by=updated_at&order=desc", nil, token)
		if err != nil {
			return latest, err
		}
		if status != http.StatusOK {
			return latest, fmt.Errorf("tasks HTTP %d", status)
		}
		raw, _ := json.Marshal(data["tasks"])
		var rows []taskInfo
		_ = json.Unmarshal(raw, &rows)
		for _, row := range rows {
			if taskIDs[row.TaskID] {
				latest[row.TaskID] = row
			}
		}
		terminal := 0
		for _, task := range tasks {
			if row, ok := latest[task.TaskID]; ok && (row.Status == "COMPLETED" || row.Status == "FAILED" || row.Status == "STOPPED") {
				terminal++
			}
		}
		rawLatest, _ := json.MarshalIndent(latest, "", "  ")
		_ = os.WriteFile(filepath.Join(runDir, "tasks-latest.json"), rawLatest, 0o644)
		if terminal == len(tasks) {
			return latest, nil
		}
		time.Sleep(5 * time.Second)
	}
	return latest, nil
}

func (h *harness) evaluateRun(tasks []workloadTask, observed map[string]taskInfo, result *runResult) {
	for _, task := range tasks {
		item, ok := observed[task.TaskID]
		if !ok {
			result.Failures = append(result.Failures, "lost task: "+task.TaskID)
			continue
		}
		if item.Status != task.ExpectedStatus {
			result.Failures = append(result.Failures, fmt.Sprintf("task %s expected %s got %s: %s", task.TaskID, task.ExpectedStatus, item.Status, item.StatusMessage))
		}
		if task.ExpectedStatus == "COMPLETED" && !strings.Contains(item.ResultTorrent, task.BTIH) {
			result.Failures = append(result.Failures, "task "+task.TaskID+" result btih mismatch")
		}
		if item.Status == "PENDING" || item.Status == "DISPATCHED" || item.Status == "RUNNING" {
			result.Failures = append(result.Failures, "stuck non-terminal task: "+task.TaskID)
		}
	}
	ids := make([]string, 0, len(result.WorkersFinal))
	for _, worker := range result.WorkersFinal {
		ids = append(ids, worker.ID)
		if worker.Status != "ACTIVE" {
			result.Failures = append(result.Failures, "inactive worker after reconnect: "+worker.ID+" status="+worker.Status)
		}
	}
	sort.Strings(ids)
	if strings.Join(ids, ",") != "worker1,worker2,worker3" {
		result.Failures = append(result.Failures, "worker leak or missing worker: "+strings.Join(ids, ","))
	}
}

func (h *harness) cleanupRuntime() {
	for _, proc := range h.processes {
		proc.stop()
	}
	h.processes = map[string]*managedProcess{}
	for _, proxy := range h.proxies {
		proxy.Stop()
	}
	h.proxies = nil
	_ = exec.Command("docker", "rm", "-f", "hivemind-rel-pg", "hivemind-rel-redis").Run()
}

func (h *harness) cleanup() {
	h.cleanupRuntime()
}

func (h *harness) writeReports() {
	raw, _ := json.MarshalIndent(h.summary, "", "  ")
	_ = os.WriteFile(filepath.Join(h.runRoot, "summary.json"), raw, 0o644)
	_ = os.WriteFile(filepath.Join(h.root, "reliability_report.md"), []byte(renderReport(h.summary)), 0o644)
}

func renderReport(s summary) string {
	var b strings.Builder
	b.WriteString("# Reliability Report\n\n")
	b.WriteString("- Generated: " + nowISO() + "\n")
	b.WriteString("- Artifacts: `" + s.Artifacts + "`\n")
	b.WriteString(fmt.Sprintf("- Runs requested: %d\n", s.Parameters.Runs))
	b.WriteString(fmt.Sprintf("- Runs completed: %d\n", len(s.Runs)))
	b.WriteString(fmt.Sprintf("- Long-running seconds configured: %d\n", s.Parameters.LongSeconds))
	b.WriteString(fmt.Sprintf("- Network latency/jitter: %dms + 0..%dms\n", s.Parameters.LatencyMS, s.Parameters.JitterMS))
	b.WriteString(fmt.Sprintf("- Overall pass: %v\n", s.Passed))
	b.WriteString(fmt.Sprintf("- DoD satisfied: %v\n\n", s.DODSatisfied))
	b.WriteString("## Run Results\n")
	for _, run := range s.Runs {
		b.WriteString(fmt.Sprintf("- Run %d `%s`: passed=%v failures=%d\n", run.Run, run.RunID, run.Passed, len(run.Failures)))
	}
	return b.String()
}

func parseArgs(argv []string) harnessArgs {
	fs := flag.NewFlagSet("reliability-harness", flag.ExitOnError)
	args := harnessArgs{}
	fs.StringVar(&args.RepoRoot, "repo-root", defaultRepoRoot(), "repository root")
	fs.IntVar(&args.Runs, "runs", 10, "number of consecutive runs")
	fs.IntVar(&args.FailureSimulations, "failure-simulations", 3, "node failure simulations per run")
	fs.IntVar(&args.LongSeconds, "long-seconds", 900, "long workload seconds")
	fs.IntVar(&args.RetrySeconds, "retry-seconds", 90, "retry workload seconds")
	fs.IntVar(&args.CPUSeconds, "cpu-seconds", 5, "CPU workload seconds")
	fs.IntVar(&args.IOSeconds, "io-seconds", 5, "IO workload seconds")
	fs.IntVar(&args.FailureSeconds, "failure-seconds", 3, "failure workload delay seconds")
	fs.IntVar(&args.ParallelSeconds, "parallel-seconds", 10, "parallel workload seconds")
	fs.IntVar(&args.ParallelTasks, "parallel-tasks", 6, "parallel task count")
	fs.IntVar(&args.LatencyMS, "latency-ms", 100, "proxy latency milliseconds")
	fs.IntVar(&args.JitterMS, "jitter-ms", 250, "proxy jitter milliseconds")
	fs.StringVar(&args.WorkerNodepoolAddr, "worker-nodepool-addr", "127.0.0.1:55051", "nodepool addr used by workers")
	fs.IntVar(&args.TaskTimeoutSeconds, "task-timeout-seconds", 20, "nodepool task timeout seconds")
	fs.IntVar(&args.TaskWaitTimeoutSeconds, "task-wait-timeout-seconds", 1500, "task wait timeout seconds")
	fs.IntVar(&args.KillMinSeconds, "kill-min-seconds", 5, "min seconds before killing a worker")
	fs.IntVar(&args.KillMaxSeconds, "kill-max-seconds", 30, "max seconds before killing a worker")
	fs.IntVar(&args.ReconnectMinSeconds, "reconnect-min-seconds", 5, "min seconds before reconnect")
	fs.IntVar(&args.ReconnectMaxSeconds, "reconnect-max-seconds", 30, "max seconds before reconnect")
	fs.Int64Var(&args.Seed, "seed", 20260520, "random seed")
	fs.BoolVar(&args.StopOnFailure, "stop-on-failure", false, "stop after first failed run")
	fs.BoolVar(&args.Calibration, "calibration", false, "short non-DoD calibration run")
	_ = fs.Parse(argv)
	if args.Calibration {
		args.Runs = 1
		args.FailureSimulations = 1
		if args.LongSeconds > 30 {
			args.LongSeconds = 30
		}
		if args.RetrySeconds > 35 {
			args.RetrySeconds = 35
		}
		if args.TaskTimeoutSeconds > 10 {
			args.TaskTimeoutSeconds = 10
		}
		if args.TaskWaitTimeoutSeconds > 240 {
			args.TaskWaitTimeoutSeconds = 240
		}
		if args.ParallelTasks > 3 {
			args.ParallelTasks = 3
		}
	}
	return args
}

func main() {
	if _, err := exec.LookPath("docker"); err != nil {
		fmt.Fprintln(os.Stderr, "docker is required")
		os.Exit(2)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	_ = ctx
	os.Exit(newHarness(parseArgs(os.Args[1:])).run())
}

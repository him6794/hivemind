package main

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/pbnjay/memory"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"hivemind/services/worker/internal/bt"
	"hivemind/services/worker/internal/service"
	"hivemind/services/worker/pb"
)

type workerServer struct {
	pb.UnimplementedWorkerNodeServiceServer
	svc *service.TaskService
	rt  *workerRuntime
}

type workerRuntime struct {
	nodepoolAddr string
	workerName   string
	workerAddr   string
	profile      workerProfile

	mu          sync.Mutex
	taskStopped map[string]bool
	taskCancel  map[string]context.CancelFunc
	authToken   string

	dialContext func(context.Context, string) (*grpc.ClientConn, error)
}

type workerProfile struct {
	IP          string `json:"ip"`
	CpuCores    int32  `json:"cpu_cores"`
	MemoryGb    int32  `json:"memory_gb"`
	CpuScore    int32  `json:"cpu_score"`
	GpuScore    int32  `json:"gpu_score"`
	GpuMemoryGb int32  `json:"gpu_memory_gb"`
	Location    string `json:"location"`
}

func newWorkerRuntime(nodepoolAddr, workerName, workerAddr string) *workerRuntime {
	cpuCores := int32(runtime.NumCPU())
	if cpuCores <= 0 {
		cpuCores = 4
	}
	memoryGb := int32(memory.TotalMemory() / 1024 / 1024 / 1024)
	if memoryGb <= 0 {
		memoryGb = 8
	}
	cpuScore := cpuCores * 20
	if cpuScore < 50 {
		cpuScore = 50
	}
	gpuScore := int32(float32(cpuScore) * 0.8)
	if gpuScore < 40 {
		gpuScore = 40
	}
	gpuMemoryGb := memoryGb / 2
	if gpuMemoryGb < 2 {
		gpuMemoryGb = 2
	}
	loc := os.Getenv("WORKER_LOCATION")
	if strings.TrimSpace(loc) == "" {
		loc = time.Now().Location().String()
	}

	return &workerRuntime{
		nodepoolAddr: nodepoolAddr,
		workerName:   workerName,
		workerAddr:   workerAddr,
		profile: workerProfile{
			IP:          workerAddr,
			CpuCores:    cpuCores,
			MemoryGb:    memoryGb,
			CpuScore:    cpuScore,
			GpuScore:    gpuScore,
			GpuMemoryGb: gpuMemoryGb,
			Location:    loc,
		},
		taskStopped: make(map[string]bool),
		taskCancel:  make(map[string]context.CancelFunc),
	}
}

func (r *workerRuntime) getProfile() workerProfile {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.profile
}

func (r *workerRuntime) markStopped(taskID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.taskStopped[taskID] = true
	if cancel, ok := r.taskCancel[taskID]; ok && cancel != nil {
		cancel()
	}
}

func (r *workerRuntime) isStopped(taskID string) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.taskStopped[taskID]
}

func (r *workerRuntime) clearStopped(taskID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	delete(r.taskStopped, taskID)
}

func (r *workerRuntime) setTaskCancel(taskID string, cancel context.CancelFunc) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.taskCancel[taskID] = cancel
}

func (r *workerRuntime) clearTaskCancel(taskID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	delete(r.taskCancel, taskID)
}

func (r *workerRuntime) dialNodepool(ctx context.Context) (*grpc.ClientConn, error) {
	if r.dialContext != nil {
		return r.dialContext(ctx, r.nodepoolAddr)
	}
	return grpc.DialContext(ctx, r.nodepoolAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
}

func (r *workerRuntime) withNodepool(ctx context.Context, fn func(pb.NodeManagerServiceClient, pb.WorkerNodeServiceClient) error) error {
	conn, err := r.dialNodepool(ctx)
	if err != nil {
		return err
	}
	defer conn.Close()
	return fn(pb.NewNodeManagerServiceClient(conn), pb.NewWorkerNodeServiceClient(conn))
}

func (r *workerRuntime) getWorkerAuthToken(ctx context.Context) string {
	r.mu.Lock()
	cached := strings.TrimSpace(r.authToken)
	r.mu.Unlock()
	if cached != "" {
		return cached
	}

	password := strings.TrimSpace(os.Getenv("WORKER_PASSWORD"))
	if password == "" {
		password = "worker123"
	}
	conn, err := r.dialNodepool(ctx)
	if err != nil {
		return ""
	}
	defer conn.Close()
	client := pb.NewUserServiceClient(conn)
	resp, err := client.Login(ctx, &pb.LoginRequest{Username: r.workerName, Password: password})
	if err != nil || !resp.GetSuccess() {
		return ""
	}
	tok := strings.TrimSpace(resp.GetToken())
	if tok == "" {
		return ""
	}
	r.mu.Lock()
	r.authToken = tok
	r.mu.Unlock()
	return tok
}

func envInt32(name string, fallback int32) int32 {
	v := os.Getenv(name)
	if v == "" {
		return fallback
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		return fallback
	}
	return int32(n)
}

func (r *workerRuntime) registerOnce(ctx context.Context) error {
	p := r.getProfile()
	return r.withNodepool(ctx, func(nodeClient pb.NodeManagerServiceClient, _ pb.WorkerNodeServiceClient) error {
		resp, err := nodeClient.RegisterWorkerNode(ctx, &pb.RegisterWorkerNodeRequest{
			Username:    r.workerName,
			Ip:          p.IP,
			CpuCores:    p.CpuCores,
			MemoryGb:    p.MemoryGb,
			CpuScore:    p.CpuScore,
			GpuScore:    p.GpuScore,
			GpuMemoryGb: p.GpuMemoryGb,
			Location:    p.Location,
		})
		if err != nil {
			return err
		}
		if !resp.GetSuccess() {
			return fmt.Errorf("register failed: %s", resp.GetStatusMessage())
		}
		return nil
	})
}

func startControlHTTP(rt *workerRuntime) {
	addr := os.Getenv("WORKER_CONTROL_ADDR")
	if addr == "" {
		addr = ":18080"
	}
	requestLogMiddleware := func(serviceName string, next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			start := time.Now()
			reqID := strings.TrimSpace(r.Header.Get("X-Request-Id"))
			if reqID == "" {
				reqID = fmt.Sprintf("%d", time.Now().UnixNano())
			}
			w.Header().Set("X-Request-Id", reqID)
			next.ServeHTTP(w, r)
			log.Printf("service=%s request_id=%s method=%s path=%s duration_ms=%d remote=%s", serviceName, reqID, r.Method, r.URL.Path, time.Since(start).Milliseconds(), r.RemoteAddr)
		})
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/api/worker-info", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "GET, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "profile": rt.getProfile()})
	})
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "service": "worker-control"})
	})
	go func() {
		log.Printf("worker control http listening on %s", addr)
		if err := http.ListenAndServe(addr, requestLogMiddleware("worker-control-http", mux)); err != nil {
			log.Printf("worker control http stopped: %v", err)
		}
	}()
}

func (r *workerRuntime) heartbeatLoop(ctx context.Context) {
	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			hbCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
			err := r.withNodepool(hbCtx, func(nodeClient pb.NodeManagerServiceClient, _ pb.WorkerNodeServiceClient) error {
				resp, err := nodeClient.ReportStatus(hbCtx, &pb.RunningStatusRequest{
					Username:       r.workerName,
					Status:         "Idle",
					CpuUsage:       10,
					MemoryUsage:    20,
					GpuUsage:       5,
					GpuMemoryUsage: 8,
				})
				if err != nil {
					return err
				}
				if !resp.GetSuccess() {
					return fmt.Errorf("report status rejected: %s", resp.GetStatusMessage())
				}
				return nil
			})
			cancel()
			if err == nil {
				continue
			}
			log.Printf("worker heartbeat failed username=%s err=%v; trying re-register", r.workerName, err)

			regCtx, regCancel := context.WithTimeout(ctx, 3*time.Second)
			regErr := r.registerOnce(regCtx)
			regCancel()
			if regErr != nil {
				log.Printf("worker re-register failed username=%s err=%v", r.workerName, regErr)
				continue
			}

			retryCtx, retryCancel := context.WithTimeout(ctx, 2*time.Second)
			retryErr := r.withNodepool(retryCtx, func(nodeClient pb.NodeManagerServiceClient, _ pb.WorkerNodeServiceClient) error {
				resp, err := nodeClient.ReportStatus(retryCtx, &pb.RunningStatusRequest{
					Username:       r.workerName,
					Status:         "Idle",
					CpuUsage:       10,
					MemoryUsage:    20,
					GpuUsage:       5,
					GpuMemoryUsage: 8,
				})
				if err != nil {
					return err
				}
				if !resp.GetSuccess() {
					return fmt.Errorf("report status rejected after re-register: %s", resp.GetStatusMessage())
				}
				return nil
			})
			retryCancel()
			if retryErr != nil {
				log.Printf("worker heartbeat retry failed username=%s err=%v", r.workerName, retryErr)
			}
		}
	}
}

func clampPercent(v float32) float32 {
	if v < 0 {
		return 0
	}
	if v > 100 {
		return 100
	}
	return v
}

func (r *workerRuntime) generateUsageSample(taskID string) (cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage float32) {
	p := r.getProfile()
	nowBucket := float32((time.Now().UnixNano() / int64(time.Millisecond)) % 25)
	taskOffset := float32(len(taskID) % 7)

	cpuUsage = clampPercent(30 + nowBucket + taskOffset)
	memoryUsage = clampPercent(40 + nowBucket*0.8 + float32(p.MemoryGb%16))
	gpuUsage = clampPercent(20 + nowBucket*0.6 + float32(p.GpuScore%20)/2)
	gpuMemoryUsage = clampPercent(35 + nowBucket*0.7 + float32(p.GpuMemoryGb%12))
	return
}

func (r *workerRuntime) reportTaskUsage(taskID string, cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage float32) {
	if strings.TrimSpace(taskID) == "" {
		return
	}
	usageCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	tok := r.getWorkerAuthToken(usageCtx)
	_ = r.withNodepool(usageCtx, func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
		_, err := workerClient.TaskUsage(usageCtx, &pb.TaskUsageRequest{
			TaskId:         taskID,
			CpuUsage:       cpuUsage,
			MemoryUsage:    memoryUsage,
			GpuUsage:       gpuUsage,
			GpuMemoryUsage: gpuMemoryUsage,
			Token:          tok,
		})
		return err
	})
}

func (r *workerRuntime) uploadTaskOutput(ctx context.Context, workerClient pb.WorkerNodeServiceClient, taskID, output string) {
	tok := r.getWorkerAuthToken(ctx)
	_, _ = workerClient.TaskOutputUpload(ctx, &pb.TaskOutputUploadRequest{TaskId: taskID, Output: output, Token: tok})
}

func (r *workerRuntime) uploadTaskResult(ctx context.Context, workerClient pb.WorkerNodeServiceClient, taskID, result string) {
	tok := r.getWorkerAuthToken(ctx)
	_, _ = workerClient.TaskResultUpload(ctx, &pb.TaskResultUploadRequest{TaskId: taskID, ResultTorrent: result, Token: tok})
}

func (r *workerRuntime) startTaskUsageReporter(taskID string) context.CancelFunc {
	intervalSec := envInt32("WORKER_USAGE_REPORT_INTERVAL_SEC", 2)
	if intervalSec <= 0 {
		intervalSec = 2
	}

	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		ticker := time.NewTicker(time.Duration(intervalSec) * time.Second)
		defer ticker.Stop()

		cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage := r.generateUsageSample(taskID)
		r.reportTaskUsage(taskID, cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage)

		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if r.isStopped(taskID) {
					return
				}
				cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage = r.generateUsageSample(taskID)
				r.reportTaskUsage(taskID, cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage)
			}
		}
	}()

	return cancel
}

func (r *workerRuntime) executeAndUpload(taskID, torrent string) {
	r.clearStopped(taskID)
	hash, source, name, validateErr := r.resolveTorrentSource(torrent)
	stopUsageReporter := r.startTaskUsageReporter(taskID)
	defer stopUsageReporter()

	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()

	_ = r.withNodepool(ctx, func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
		msg := "task accepted by worker"
		if validateErr == nil {
			msg = fmt.Sprintf("task accepted (%s, btih=%s, name=%s)", source, hash, name)
		} else {
			msg = fmt.Sprintf("task accepted (unverified source): %v", validateErr)
		}
		r.uploadTaskOutput(ctx, workerClient, taskID, msg)
		return nil
	})

	result := ""
	runRes, err := r.runExternalExecutor(taskID, torrent)
	if len(runRes.outputLines) > 0 {
		_ = r.withNodepool(context.Background(), func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
			for _, line := range runRes.outputLines {
				r.uploadTaskOutput(context.Background(), workerClient, taskID, line)
			}
			return nil
		})
	}
	if err != nil {
		if r.isStopped(taskID) {
			_ = r.withNodepool(context.Background(), func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
				r.uploadTaskOutput(context.Background(), workerClient, taskID, "task stopped")
				return nil
			})
			return
		}
		_ = r.withNodepool(context.Background(), func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
			r.uploadTaskOutput(context.Background(), workerClient, taskID, fmt.Sprintf("task failed: %v", err))
			return nil
		})
		return
	}
	result = runRes.result

	if r.isStopped(taskID) {
		_ = r.withNodepool(context.Background(), func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
			r.uploadTaskOutput(context.Background(), workerClient, taskID, "task stopped")
			return nil
		})
		return
	}

	cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage := r.generateUsageSample(taskID)
	r.reportTaskUsage(taskID, cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage)

	_ = r.withNodepool(context.Background(), func(_ pb.NodeManagerServiceClient, workerClient pb.WorkerNodeServiceClient) error {
		r.uploadTaskOutput(context.Background(), workerClient, taskID, "task completed")
		r.uploadTaskResult(context.Background(), workerClient, taskID, result)
		return nil
	})
}

type executorRunResult struct {
	result      string
	outputLines []string
}

func parseExecutorResult(output string) string {
	lines := strings.Split(output, "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		line := strings.TrimSpace(lines[i])
		if line == "" {
			continue
		}
		if strings.HasPrefix(line, "RESULT_TORRENT=") {
			v := strings.TrimSpace(strings.TrimPrefix(line, "RESULT_TORRENT="))
			if v != "" {
				return v
			}
		}
		if strings.HasPrefix(strings.ToLower(line), "magnet:?") || strings.HasPrefix(strings.ToLower(line), "result://") {
			return line
		}
	}
	return ""
}

func parseEnvBool(name string, fallback bool) bool {
	v := strings.TrimSpace(strings.ToLower(os.Getenv(name)))
	if v == "" {
		return fallback
	}
	switch v {
	case "1", "true", "yes", "y", "on":
		return true
	case "0", "false", "no", "n", "off":
		return false
	default:
		return fallback
	}
}

func resolveExecutorCommand() string {
	if cmd := strings.TrimSpace(os.Getenv("WORKER_EXECUTOR_CMD")); cmd != "" {
		return cmd
	}
	if !parseEnvBool("WORKER_EXECUTOR_AUTO_RUST", true) {
		return ""
	}
	bin := strings.TrimSpace(os.Getenv("WORKER_EXECUTOR_RS_BIN"))
	if bin == "" {
		bin = defaultExecutorCommand()
	}
	return bin
}

func defaultExecutorCommand() string {
	for _, candidate := range repoExecutorCandidates() {
		if _, err := os.Stat(candidate); err == nil {
			return candidate
		}
	}
	return "executor-cli"
}

func repoExecutorCandidates() []string {
	exeName := "monty"
	cliName := "executor-cli"
	if runtime.GOOS == "windows" {
		exeName += ".exe"
		cliName += ".exe"
	}

	candidates := []string{}
	if wd, err := os.Getwd(); err == nil {
		for dir := wd; ; dir = filepath.Dir(dir) {
			candidates = append(candidates,
				filepath.Join(dir, "executor-rs", exeName),
				filepath.Join(dir, "executor-rs", "target", "debug", cliName),
				filepath.Join(dir, "executor-rs", "target", "release", cliName),
			)
			parent := filepath.Dir(dir)
			if parent == dir {
				break
			}
		}
	}
	return candidates
}

func executorProgramFromCommand(cmdline string) (string, error) {
	parts := strings.Fields(strings.TrimSpace(cmdline))
	if len(parts) == 0 {
		return "", fmt.Errorf("empty executor command")
	}
	return parts[0], nil
}

func checkExecutorAvailability() (cmdline string, program string, resolvedPath string, err error) {
	cmdline = resolveExecutorCommand()
	if strings.TrimSpace(cmdline) == "" {
		return "", "", "", fmt.Errorf("executor command not configured")
	}
	program, err = executorProgramFromCommand(cmdline)
	if err != nil {
		return cmdline, "", "", err
	}
	resolvedPath, err = exec.LookPath(program)
	if err != nil {
		return cmdline, program, "", err
	}
	return cmdline, program, resolvedPath, nil
}

func executorOutputLines(output string) []string {
	text := strings.ReplaceAll(output, "\r\n", "\n")
	lines := strings.Split(text, "\n")
	res := make([]string, 0, len(lines))
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		if strings.HasPrefix(line, "RESULT_TORRENT=") {
			continue
		}
		res = append(res, line)
	}
	return res
}

func (r *workerRuntime) runExternalExecutor(taskID, torrent string) (executorRunResult, error) {
	cmdline := resolveExecutorCommand()
	if cmdline == "" {
		return executorRunResult{}, fmt.Errorf("executor command not configured")
	}
	parts := strings.Fields(cmdline)
	if len(parts) == 0 {
		return executorRunResult{}, fmt.Errorf("invalid executor command")
	}
	if isMontyExecutor(parts[0]) {
		return r.runMontyExecutor(parts, taskID, torrent)
	}
	timeoutSec := envInt32("WORKER_EXECUTOR_TIMEOUT_SEC", 120)
	if timeoutSec <= 0 {
		timeoutSec = 120
	}

	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutSec)*time.Second)
	r.setTaskCancel(taskID, cancel)
	defer func() {
		cancel()
		r.clearTaskCancel(taskID)
	}()

	args := make([]string, 0, len(parts)-1+2)
	if len(parts) > 1 {
		args = append(args, parts[1:]...)
	}
	args = append(args, taskID, torrent)

	cmd := exec.CommandContext(ctx, parts[0], args...)
	out, err := cmd.CombinedOutput()
	outText := string(out)
	outputLines := executorOutputLines(outText)

	if r.isStopped(taskID) {
		return executorRunResult{outputLines: outputLines}, fmt.Errorf("task stopped")
	}
	if ctx.Err() == context.DeadlineExceeded {
		return executorRunResult{outputLines: outputLines}, fmt.Errorf("executor timeout after %ds", timeoutSec)
	}
	if err != nil {
		return executorRunResult{outputLines: outputLines}, fmt.Errorf("executor failed: %v", err)
	}

	if result := parseExecutorResult(outText); result != "" {
		return executorRunResult{result: result, outputLines: outputLines}, nil
	}
	return executorRunResult{outputLines: outputLines}, fmt.Errorf("executor completed but no RESULT_TORRENT/magnet/result:// output")
}

func isMontyExecutor(program string) bool {
	base := strings.ToLower(filepath.Base(strings.TrimSpace(program)))
	return base == "monty" || base == "monty.exe"
}

func isHexBTIH(hash string) bool {
	hash = strings.TrimSpace(hash)
	if len(hash) != 40 {
		return false
	}
	_, err := hex.DecodeString(hash)
	return err == nil
}

func btihFromTorrentSource(torrent string) string {
	raw := strings.TrimSpace(torrent)
	if raw == "" {
		return ""
	}

	u, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	q := u.Query()
	if strings.EqualFold(u.Scheme, "magnet") {
		for _, xt := range q["xt"] {
			xt = strings.TrimSpace(xt)
			if len(xt) >= len("urn:btih:") && strings.EqualFold(xt[:len("urn:btih:")], "urn:btih:") {
				hash := strings.ToLower(strings.TrimSpace(xt[len("urn:btih:"):]))
				if isHexBTIH(hash) {
					return hash
				}
			}
		}
	}
	for _, key := range []string{"ih", "btih"} {
		hash := strings.ToLower(strings.TrimSpace(q.Get(key)))
		if isHexBTIH(hash) {
			return hash
		}
	}
	return ""
}

func montyResultScript(taskID, torrent string) (script string, result string) {
	btih := btihFromTorrentSource(torrent)
	if btih == "" {
		sum := sha1.Sum([]byte(taskID + "|" + torrent))
		btih = hex.EncodeToString(sum[:])
	}
	result = fmt.Sprintf("result://%s?btih=%s", url.QueryEscape(taskID), btih)
	return fmt.Sprintf("print(%q)", "RESULT_TORRENT="+result), result
}

func (r *workerRuntime) runMontyExecutor(parts []string, taskID, torrent string) (executorRunResult, error) {
	timeoutSec := envInt32("WORKER_EXECUTOR_TIMEOUT_SEC", 120)
	if timeoutSec <= 0 {
		timeoutSec = 120
	}

	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutSec)*time.Second)
	r.setTaskCancel(taskID, cancel)
	defer func() {
		cancel()
		r.clearTaskCancel(taskID)
	}()

	script, fallbackResult := montyResultScript(taskID, torrent)
	args := make([]string, 0, len(parts)+2)
	if len(parts) > 1 {
		args = append(args, parts[1:]...)
	}
	args = append(args, "-c", script)

	cmd := exec.CommandContext(ctx, parts[0], args...)
	out, err := cmd.CombinedOutput()
	outText := string(out)
	outputLines := executorOutputLines(outText)

	if r.isStopped(taskID) {
		return executorRunResult{outputLines: outputLines}, fmt.Errorf("task stopped")
	}
	if ctx.Err() == context.DeadlineExceeded {
		return executorRunResult{outputLines: outputLines}, fmt.Errorf("executor timeout after %ds", timeoutSec)
	}
	if err != nil {
		return executorRunResult{outputLines: outputLines}, fmt.Errorf("executor failed: %v", err)
	}

	if result := parseExecutorResult(outText); result != "" {
		return executorRunResult{result: result, outputLines: outputLines}, nil
	}
	return executorRunResult{result: fallbackResult, outputLines: outputLines}, nil
}

func (r *workerRuntime) batchPullCapacity() (maxInflight, availableMemory, queueCapacity int32) {
	maxInflight = envInt32("WORKER_BATCH_MAX_INFLIGHT", 1)
	if maxInflight <= 0 {
		maxInflight = 1
	}
	queueCapacity = envInt32("WORKER_BATCH_QUEUE_CAPACITY", 1)
	if queueCapacity <= 0 {
		queueCapacity = 1
	}
	availableMemory = envInt32("WORKER_BATCH_AVAILABLE_MEMORY_GB", 0)
	if availableMemory <= 0 {
		availableMemory = r.getProfile().MemoryGb
	}
	if availableMemory <= 0 {
		availableMemory = 1
	}
	return maxInflight, availableMemory, queueCapacity
}

func leaseCodeRef(lease *pb.TaskLease) string {
	if lease == nil {
		return ""
	}
	if pkg := lease.GetExecutionPackage(); pkg != nil {
		if ref := strings.TrimSpace(pkg.GetTaskCodeRef()); ref != "" {
			return ref
		}
		for _, ref := range pkg.GetArtifactRefs() {
			if ref = strings.TrimSpace(ref); ref != "" {
				return ref
			}
		}
	}
	for _, manifest := range lease.GetArtifacts() {
		if ref := strings.TrimSpace(manifest.GetArtifactId()); ref != "" {
			return ref
		}
	}
	return ""
}

func leaseDownloadBytes(lease *pb.TaskLease) int64 {
	if lease == nil {
		return 0
	}
	var total int64
	if pkg := lease.GetExecutionPackage(); pkg != nil {
		total += int64(len(pkg.GetTaskCodeRef()))
		for _, ref := range pkg.GetArtifactRefs() {
			total += int64(len(ref))
		}
	}
	for _, manifest := range lease.GetArtifacts() {
		if manifest.GetSize() > 0 {
			total += manifest.GetSize()
			continue
		}
		total += int64(len(manifest.GetArtifactId()))
	}
	return total
}

func (r *workerRuntime) executeBatchLease(ctx context.Context, lease *pb.TaskLease) *pb.CompletedTask {
	start := time.Now()
	taskID := strings.TrimSpace(lease.GetTaskId())
	codeRef := leaseCodeRef(lease)

	completed := &pb.CompletedTask{
		TaskId: taskID,
		Status: "FAILED",
		Metrics: &pb.ExecutionMetrics{
			DownloadBytes: leaseDownloadBytes(lease),
		},
	}
	defer func() {
		wallMS := time.Since(start).Milliseconds()
		if wallMS <= 0 {
			wallMS = 1
		}
		completed.Metrics.WallTimeMs = wallMS
		completed.Metrics.CpuTimeMs = wallMS
		if limits := lease.GetResourceLimits(); limits != nil && limits.GetMemoryGb() > 0 {
			completed.Metrics.PeakMemoryMb = int64(limits.GetMemoryGb()) * 1024
		}
	}()

	if taskID == "" {
		completed.StderrArtifactRef = "error://missing-task-id"
		return completed
	}
	if err := ctx.Err(); err != nil {
		completed.StderrArtifactRef = "error://" + url.QueryEscape(err.Error())
		return completed
	}
	if codeRef == "" {
		completed.StderrArtifactRef = "error://missing-task-code-ref"
		return completed
	}

	r.clearStopped(taskID)
	runRes, err := r.runExternalExecutor(taskID, codeRef)
	if len(runRes.outputLines) > 0 {
		completed.StdoutArtifactRef = "artifact://stdout/" + url.QueryEscape(taskID)
	}
	if err != nil {
		completed.Status = "FAILED"
		completed.StderrArtifactRef = "error://" + url.QueryEscape(err.Error())
		return completed
	}
	completed.Status = "COMPLETED"
	if runRes.result != "" {
		completed.ResultArtifactRefs = []string{runRes.result}
	}
	return completed
}

func (r *workerRuntime) pullBatchOnce(ctx context.Context) (bool, error) {
	maxInflight, availableMemory, queueCapacity := r.batchPullCapacity()
	conn, err := r.dialNodepool(ctx)
	if err != nil {
		return false, err
	}
	defer conn.Close()

	client := pb.NewBatchRuntimeServiceClient(conn)
	rpcTimeoutSec := envInt32("WORKER_BATCH_RPC_TIMEOUT_SEC", 10)
	if rpcTimeoutSec <= 0 {
		rpcTimeoutSec = 10
	}
	pullCtx, pullCancel := context.WithTimeout(ctx, time.Duration(rpcTimeoutSec)*time.Second)
	resp, err := client.PullBatch(pullCtx, &pb.PullBatchRequest{
		WorkerId:           r.workerName,
		MaxInflightBatches: maxInflight,
		AvailableMemoryGb:  availableMemory,
		QueueCapacity:      queueCapacity,
		CacheSummary:       &pb.CacheSummary{},
	})
	pullCancel()
	if err != nil {
		return false, err
	}
	if !resp.GetSuccess() {
		return false, fmt.Errorf("pull batch rejected: %s", resp.GetStatusMessage())
	}
	if len(resp.GetTasks()) == 0 {
		return false, nil
	}

	completed := make([]*pb.CompletedTask, 0, len(resp.GetTasks()))
	for _, lease := range resp.GetTasks() {
		completed = append(completed, r.executeBatchLease(ctx, lease))
	}

	completeCtx, completeCancel := context.WithTimeout(ctx, time.Duration(rpcTimeoutSec)*time.Second)
	completeResp, err := client.CompleteBatch(completeCtx, &pb.CompleteBatchRequest{
		WorkerId: r.workerName,
		BatchId:  resp.GetBatchId(),
		Tasks:    completed,
	})
	completeCancel()
	if err != nil {
		return true, err
	}
	if !completeResp.GetSuccess() {
		return true, fmt.Errorf("complete batch rejected: %s", completeResp.GetStatusMessage())
	}
	return true, nil
}

func (r *workerRuntime) pullBatchLoop(ctx context.Context) {
	intervalSec := envInt32("WORKER_BATCH_POLL_INTERVAL_SEC", 2)
	if intervalSec <= 0 {
		intervalSec = 2
	}
	interval := time.Duration(intervalSec) * time.Second

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		handled, err := r.pullBatchOnce(ctx)
		if err != nil {
			log.Printf("worker batch pull failed username=%s err=%v", r.workerName, err)
		}
		if handled && err == nil {
			continue
		}

		timer := time.NewTimer(interval)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
	}
}

func (r *workerRuntime) resolveTorrentSource(torrent string) (infoHash string, source string, name string, err error) {
	torrent = strings.TrimSpace(torrent)
	if torrent == "" {
		return "", "", "", fmt.Errorf("empty torrent source")
	}
	if mg, e := bt.ParseMagnet(torrent); e == nil {
		return mg.InfoHash, "magnet", mg.DisplayName, nil
	}
	if strings.HasPrefix(strings.ToLower(torrent), "http://") || strings.HasPrefix(strings.ToLower(torrent), "https://") {
		h, n, e := r.fetchAndValidateTorrentURL(torrent)
		return h, "torrent-url", n, e
	}
	return "", "unknown", "", fmt.Errorf("unsupported torrent source")
}

func (r *workerRuntime) fetchAndValidateTorrentURL(rawURL string) (infoHash string, name string, err error) {
	ctx, cancel := context.WithTimeout(context.Background(), 6*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return "", "", err
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", "", err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", "", fmt.Errorf("fetch torrent failed status=%d", resp.StatusCode)
	}
	b, err := io.ReadAll(io.LimitReader(resp.Body, 16*1024*1024))
	if err != nil {
		return "", "", err
	}
	hash, err := bt.ParseTorrentInfoHash(b)
	if err != nil {
		return "", "", err
	}
	u, err := url.Parse(rawURL)
	if err == nil {
		expected := strings.ToLower(strings.TrimSpace(u.Query().Get("ih")))
		if expected != "" && expected != hash {
			return "", "", fmt.Errorf("info-hash mismatch expected=%s got=%s", expected, hash)
		}
		name = filepath.Base(u.Path)
	}
	if strings.TrimSpace(name) == "" {
		name = "payload.torrent"
	}
	return hash, name, nil
}

func (w *workerServer) ExecuteTask(ctx context.Context, req *pb.ExecuteTaskRequest) (*pb.ExecuteTaskResponse, error) {
	err := w.svc.ExecuteTask(ctx, req.GetTaskId(), req.GetTorrent(), req.GetCpuUsage(), req.GetGpuUsage(), req.GetMemoryGb(), req.GetGpuMemoryGb())
	if err != nil {
		return &pb.ExecuteTaskResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if w.rt != nil {
		go w.rt.executeAndUpload(req.GetTaskId(), req.GetTorrent())
	}
	return &pb.ExecuteTaskResponse{Success: true, StatusMessage: "accepted"}, nil
}

func (w *workerServer) TaskOutputUpload(ctx context.Context, req *pb.TaskOutputUploadRequest) (*pb.TaskOutputUploadResponse, error) {
	err := w.svc.UploadOutput(ctx, req.GetTaskId(), req.GetOutput())
	if err != nil {
		return &pb.TaskOutputUploadResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.TaskOutputUploadResponse{Success: true, StatusMessage: "uploaded"}, nil
}

func (w *workerServer) TaskResultUpload(ctx context.Context, req *pb.TaskResultUploadRequest) (*pb.TaskResultUploadResponse, error) {
	err := w.svc.UploadResult(ctx, req.GetTaskId(), req.GetResultTorrent())
	if err != nil {
		return &pb.TaskResultUploadResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.TaskResultUploadResponse{Success: true, StatusMessage: "uploaded"}, nil
}

func (w *workerServer) TaskOutput(ctx context.Context, req *pb.TaskOutputRequest) (*pb.TaskOutputResponse, error) {
	out, err := w.svc.GetOutput(ctx, req.GetTaskId())
	if err != nil {
		return &pb.TaskOutputResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.TaskOutputResponse{Success: true, StatusMessage: "ok", Output: out}, nil
}

func (w *workerServer) StopTaskExecution(ctx context.Context, req *pb.StopTaskExecutionRequest) (*pb.StopTaskExecutionResponse, error) {
	err := w.svc.StopTask(ctx, req.GetTaskId())
	if err != nil {
		return &pb.StopTaskExecutionResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if w.rt != nil {
		w.rt.markStopped(req.GetTaskId())
	}
	return &pb.StopTaskExecutionResponse{Success: true, StatusMessage: "stopped"}, nil
}

func (w *workerServer) TaskUsage(ctx context.Context, req *pb.TaskUsageRequest) (*pb.TaskUsageResponse, error) {
	err := w.svc.UpdateUsage(ctx, req.GetTaskId(), req.GetCpuUsage(), req.GetMemoryUsage(), req.GetGpuUsage(), req.GetGpuMemoryUsage())
	if err != nil {
		return &pb.TaskUsageResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.TaskUsageResponse{Success: true, StatusMessage: "updated"}, nil
}

func main() {
	addr := ":50053"
	if v := os.Getenv("WORKER_ADDR"); v != "" {
		addr = v
	}
	publicAddr := os.Getenv("WORKER_PUBLIC_ADDR")
	if publicAddr == "" {
		publicAddr = addr
	}
	nodepoolAddr := os.Getenv("NODEPOOL_ADDR")
	if nodepoolAddr == "" {
		nodepoolAddr = "localhost:50051"
	}
	workerName := os.Getenv("WORKER_USERNAME")
	if workerName == "" {
		workerName = "worker1"
	}

	lis, err := net.Listen("tcp", addr)
	if err != nil {
		log.Fatalf("failed to listen: %v", err)
	}

	runtime := newWorkerRuntime(nodepoolAddr, workerName, publicAddr)
	startControlHTTP(runtime)
	if cmdline, program, resolvedPath, err := checkExecutorAvailability(); err != nil {
		if strings.TrimSpace(cmdline) == "" {
			log.Printf("worker executor disabled; fallback simulation path is active")
		} else {
			log.Printf("worker executor command configured but unavailable: cmd=%q program=%q err=%v", cmdline, program, err)
		}
	} else {
		log.Printf("worker executor ready: cmd=%q program=%q resolved=%q", cmdline, program, resolvedPath)
	}
	autoRegister := strings.EqualFold(os.Getenv("WORKER_AUTO_REGISTER"), "true") || os.Getenv("WORKER_AUTO_REGISTER") == "1"
	if autoRegister {
		regCtx, regCancel := context.WithTimeout(context.Background(), 3*time.Second)
		if err := runtime.registerOnce(regCtx); err != nil {
			log.Printf("worker register warning: %v", err)
		} else {
			log.Printf("worker registered to nodepool=%s as %s, public=%s", nodepoolAddr, workerName, publicAddr)
		}
		regCancel()
	} else {
		log.Printf("worker auto-register disabled; please login from frontend and call /api/register-worker for username=%s ip=%s", workerName, publicAddr)
	}

	hbCtx, hbCancel := context.WithCancel(context.Background())
	defer hbCancel()
	go runtime.heartbeatLoop(hbCtx)

	batchCtx, batchCancel := context.WithCancel(context.Background())
	defer batchCancel()
	if parseEnvBool("WORKER_BATCH_PULL_ENABLED", true) {
		go runtime.pullBatchLoop(batchCtx)
	}

	s := grpc.NewServer()
	pb.RegisterWorkerNodeServiceServer(s, &workerServer{svc: service.NewTaskService(), rt: runtime})

	log.Printf("worker gRPC server listening on %s", addr)
	if err := s.Serve(lis); err != nil {
		log.Fatalf("worker gRPC server failed: %v", err)
	}
}

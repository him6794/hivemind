package main

import (
	"context"
	"crypto/sha1"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/redis/go-redis/v9"
	"golang.org/x/crypto/bcrypt"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"hivemind/services/nodepool/internal/repository"
	"hivemind/services/nodepool/internal/service"
	"hivemind/services/nodepool/internal/storage"
	"hivemind/services/nodepool/pb"
)

type nodeManagerServer struct {
	svc *service.WorkerService
	pb.UnimplementedNodeManagerServiceServer
}

type taskState struct {
	TaskID         string
	Owner          string
	WorkerID       string
	WorkerIP       string
	Status         string
	StatusMessage  string
	Output         string
	ResultTorrent  string
	TorrentSource  string
	ExpectedBTIH   string
	CpuUsage       float32
	MemoryUsage    float32
	GpuUsage       float32
	GpuMemoryUsage float32

	ReqCPUScore    int32
	ReqGPUScore    int32
	ReqMemoryGB    int32
	ReqGPUMemoryGB int32
	HostCount      int32

	BillingSettled   bool
	BilledAmount     int64
	RetryCount       int32
	LastUpdate       time.Time
	LastSettlementAt time.Time

	MaxRetries    int32
	Deadline      time.Time
	Deterministic bool
	SideEffects   bool
	Priority      int32

	CPUTimeMS     int64
	WallTimeMS    int64
	PeakMemoryMB  int64
	DownloadBytes int64
	CacheHits     int64
}

type batchLease struct {
	BatchID   string
	WorkerID  string
	TaskIDs   []string
	CreatedAt time.Time
}

type masterNodeServer struct {
	svc        *service.WorkerService
	auth       *userAuthServer
	db         *sql.DB
	redis      *redis.Client
	strictBTIH bool

	mu           sync.RWMutex
	taskToWorker map[string]string
	taskRoutes   map[string]map[string]string
	tasks        map[string]*taskState
	batchLeases  map[string]*batchLease
	pb.UnimplementedMasterNodeServiceServer
	pb.UnimplementedBatchRuntimeServiceServer
}

type workerIngressServer struct {
	master *masterNodeServer
	pb.UnimplementedWorkerNodeServiceServer
}

type userClaims struct {
	Username string `json:"username"`
	jwt.RegisteredClaims
}

type userAuthServer struct {
	db        *sql.DB
	jwtSecret []byte
	pb.UnimplementedUserServiceServer
}

func envInt(name string, fallback int) int {
	v := strings.TrimSpace(os.Getenv(name))
	if v == "" {
		return fallback
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		return fallback
	}
	return n
}

func envBool(name string, fallback bool) bool {
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

func envDurationSeconds(name string, fallback time.Duration) time.Duration {
	seconds := envInt(name, int(fallback/time.Second))
	if seconds <= 0 {
		return fallback
	}
	return time.Duration(seconds) * time.Second
}

type responseStatusRecorder struct {
	http.ResponseWriter
	status int
}

func (r *responseStatusRecorder) WriteHeader(code int) {
	r.status = code
	r.ResponseWriter.WriteHeader(code)
}

func requestLogMiddleware(serviceName string, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		reqID := strings.TrimSpace(r.Header.Get("X-Request-Id"))
		if reqID == "" {
			reqID = fmt.Sprintf("%d", time.Now().UnixNano())
		}
		w.Header().Set("X-Request-Id", reqID)

		rw := &responseStatusRecorder{ResponseWriter: w, status: http.StatusOK}
		next.ServeHTTP(rw, r)

		log.Printf("service=%s request_id=%s method=%s path=%s status=%d duration_ms=%d remote=%s", serviceName, reqID, r.Method, r.URL.Path, rw.status, time.Since(start).Milliseconds(), r.RemoteAddr)
	})
}

// nodepoolLog appends a timestamped message to NODEPOOL_LOG_FILE (or nodepool.log)
func nodepoolLog(msg string) {
	fname := strings.TrimSpace(os.Getenv("NODEPOOL_LOG_FILE"))
	if fname == "" {
		fname = "nodepool.log"
	}
	line := fmt.Sprintf("%s %s\n", time.Now().UTC().Format(time.RFC3339), msg)
	// write to stdout as well
	log.Print(msg)
	// append to file
	f, err := os.OpenFile(fname, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	_, _ = f.WriteString(line)
}

func hashPassword(password string) (string, error) {
	h, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return "", err
	}
	return string(h), nil
}

func verifyPassword(stored, provided string) bool {
	if strings.HasPrefix(stored, "$2") {
		return bcrypt.CompareHashAndPassword([]byte(stored), []byte(provided)) == nil
	}
	return stored == provided
}

func classifyDispatchReason(reason string) string {
	lower := strings.ToLower(strings.TrimSpace(reason))
	switch {
	case lower == "", strings.Contains(lower, "no available worker"):
		return "NO_WORKER"
	case strings.Contains(lower, "probe"):
		return "PROBE_FAIL"
	case strings.Contains(lower, "dial"):
		return "DIAL_FAIL"
	case strings.Contains(lower, "execute"):
		return "EXEC_FAIL"
	case strings.Contains(lower, "rejected"):
		return "REJECTED"
	default:
		return "DISPATCH_FAIL"
	}
}

func formatDispatchReason(reason string) string {
	reason = strings.TrimSpace(reason)
	if reason == "" {
		reason = "no available worker"
	}
	return fmt.Sprintf("[%s] %s", classifyDispatchReason(reason), reason)
}

func allowsSpeculativeExecution(node *pb.DAGNode) bool {
	return node != nil && node.GetDeterministic() && !node.GetSideEffects()
}

func (m *masterNodeServer) ensureBatchLeasesLocked() {
	if m.batchLeases == nil {
		m.batchLeases = make(map[string]*batchLease)
	}
}

func (m *masterNodeServer) inflightBatchCountLocked(workerID string) int {
	m.ensureBatchLeasesLocked()
	count := 0
	for _, lease := range m.batchLeases {
		if lease != nil && lease.WorkerID == workerID {
			count++
		}
	}
	return count
}

func taskMemoryGB(t *taskState) int32 {
	if t == nil || t.ReqMemoryGB <= 0 {
		return 1
	}
	return t.ReqMemoryGB
}

func makeTaskLease(t *taskState) *pb.TaskLease {
	deadline := int64(0)
	if !t.Deadline.IsZero() {
		deadline = t.Deadline.Unix()
	}
	artifactID := strings.TrimSpace(t.TorrentSource)
	artifacts := []*pb.ArtifactManifest{}
	if artifactID != "" {
		artifacts = append(artifacts, &pb.ArtifactManifest{
			ArtifactId:    artifactID,
			ContentType:   "application/vnd.hivemind.execution-package",
			CreatedByTask: t.TaskID,
			Compression:   "zstd",
		})
	}
	return &pb.TaskLease{
		TaskId: t.TaskID,
		ExecutionPackage: &pb.ExecutionPackage{
			RuntimeVersion: "legacy-go-monty",
			TaskCodeRef:    artifactID,
			ArtifactRefs:   []string{artifactID},
			Constraints: map[string]string{
				"deterministic": fmt.Sprintf("%t", t.Deterministic),
				"side_effects":  fmt.Sprintf("%t", t.SideEffects),
			},
		},
		Artifacts: artifacts,
		ResourceLimits: &pb.ResourceRequirements{
			MemoryGb:    taskMemoryGB(t),
			CpuCores:    t.ReqCPUScore,
			GpuScore:    t.ReqGPUScore,
			GpuMemoryGb: t.ReqGPUMemoryGB,
		},
		DeadlineUnix: deadline,
		Priority:     t.Priority,
	}
}

func (m *masterNodeServer) PullBatch(ctx context.Context, req *pb.PullBatchRequest) (*pb.PullBatchResponse, error) {
	_ = ctx
	workerID := strings.TrimSpace(req.GetWorkerId())
	if workerID == "" {
		return &pb.PullBatchResponse{Success: false, StatusMessage: "worker_id is required"}, nil
	}
	if _, ok := m.svc.GetWorker(context.Background(), workerID); !ok {
		return &pb.PullBatchResponse{Success: false, StatusMessage: "worker not registered"}, nil
	}
	if req.GetMaxInflightBatches() <= 0 || req.GetQueueCapacity() <= 0 || req.GetAvailableMemoryGb() <= 0 {
		return &pb.PullBatchResponse{Success: true, StatusMessage: "backpressure: no capacity"}, nil
	}

	m.mu.Lock()
	defer m.mu.Unlock()
	m.ensureBatchLeasesLocked()
	if m.inflightBatchCountLocked(workerID) >= int(req.GetMaxInflightBatches()) {
		return &pb.PullBatchResponse{Success: true, StatusMessage: "backpressure: inflight limit reached"}, nil
	}

	candidates := make([]*taskState, 0, len(m.tasks))
	for _, t := range m.tasks {
		if t == nil || t.TaskID == "" || t.Status != "PENDING" {
			continue
		}
		candidates = append(candidates, t)
	}
	sort.SliceStable(candidates, func(i, j int) bool {
		if candidates[i].Priority == candidates[j].Priority {
			return candidates[i].TaskID < candidates[j].TaskID
		}
		return candidates[i].Priority > candidates[j].Priority
	})

	remainingMemory := req.GetAvailableMemoryGb()
	remainingSlots := req.GetQueueCapacity()
	leases := make([]*pb.TaskLease, 0, remainingSlots)
	taskIDs := make([]string, 0, remainingSlots)
	for _, t := range candidates {
		if remainingSlots <= 0 {
			break
		}
		neededMemory := taskMemoryGB(t)
		if neededMemory > remainingMemory {
			continue
		}
		t.Status = "DISPATCHED"
		t.WorkerID = workerID
		t.StatusMessage = "leased via pull batch"
		t.LastUpdate = time.Now()
		leases = append(leases, makeTaskLease(t))
		taskIDs = append(taskIDs, t.TaskID)
		remainingMemory -= neededMemory
		remainingSlots--
	}

	if len(leases) == 0 {
		return &pb.PullBatchResponse{Success: true, StatusMessage: "no eligible tasks"}, nil
	}
	batchID := fmt.Sprintf("batch-%s-%d", workerID, time.Now().UnixNano())
	m.batchLeases[batchID] = &batchLease{
		BatchID:   batchID,
		WorkerID:  workerID,
		TaskIDs:   taskIDs,
		CreatedAt: time.Now(),
	}
	return &pb.PullBatchResponse{Success: true, StatusMessage: "leased", BatchId: batchID, Tasks: leases}, nil
}

func (m *masterNodeServer) CompleteBatch(ctx context.Context, req *pb.CompleteBatchRequest) (*pb.CompleteBatchResponse, error) {
	_ = ctx
	workerID := strings.TrimSpace(req.GetWorkerId())
	batchID := strings.TrimSpace(req.GetBatchId())
	if workerID == "" || batchID == "" {
		return &pb.CompleteBatchResponse{Success: false, StatusMessage: "worker_id and batch_id are required"}, nil
	}

	m.mu.Lock()
	defer m.mu.Unlock()
	m.ensureBatchLeasesLocked()
	lease, ok := m.batchLeases[batchID]
	if !ok || lease == nil || lease.WorkerID != workerID {
		return &pb.CompleteBatchResponse{Success: false, StatusMessage: "batch lease not found"}, nil
	}

	leased := make(map[string]bool, len(lease.TaskIDs))
	for _, id := range lease.TaskIDs {
		leased[id] = true
	}
	now := time.Now()
	for _, completed := range req.GetTasks() {
		taskID := strings.TrimSpace(completed.GetTaskId())
		if !leased[taskID] {
			continue
		}
		t, ok := m.tasks[taskID]
		if !ok || t == nil {
			continue
		}
		status := strings.ToUpper(strings.TrimSpace(completed.GetStatus()))
		if status == "" {
			status = "COMPLETED"
		}
		t.Status = status
		t.StatusMessage = "completed via pull batch"
		if status != "COMPLETED" {
			t.StatusMessage = "failed via pull batch"
		}
		t.Output = completed.GetStdoutArtifactRef()
		if len(completed.GetResultArtifactRefs()) > 0 {
			t.ResultTorrent = completed.GetResultArtifactRefs()[0]
		}
		if metrics := completed.GetMetrics(); metrics != nil {
			t.CPUTimeMS = metrics.GetCpuTimeMs()
			t.WallTimeMS = metrics.GetWallTimeMs()
			t.PeakMemoryMB = metrics.GetPeakMemoryMb()
			t.DownloadBytes = metrics.GetDownloadBytes()
			t.CacheHits = metrics.GetCacheHits()
		}
		t.LastUpdate = now
	}
	delete(m.batchLeases, batchID)
	return &pb.CompleteBatchResponse{Success: true, StatusMessage: "batch completed"}, nil
}

func (m *masterNodeServer) Heartbeat(ctx context.Context, req *pb.HeartbeatRequest) (*pb.HeartbeatResponse, error) {
	if strings.TrimSpace(req.GetWorkerId()) == "" {
		return &pb.HeartbeatResponse{Success: false, StatusMessage: "worker_id is required"}, nil
	}
	if err := m.svc.Heartbeat(ctx, req.GetWorkerId()); err != nil {
		return &pb.HeartbeatResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.HeartbeatResponse{Success: true, StatusMessage: "heartbeat recorded"}, nil
}

func initDB(dsn string) (*sql.DB, error) {
	db, err := storage.OpenPostgres(dsn)
	if err != nil {
		return nil, err
	}
	pw1, err := hashPassword("worker123")
	if err != nil {
		return nil, err
	}
	pw2, err := hashPassword("worker123")
	if err != nil {
		return nil, err
	}
	if _, err := db.Exec("INSERT INTO users(username,password,balance) VALUES($1,$2,$3) ON CONFLICT (username) DO NOTHING", "worker1", pw1, 1000); err != nil {
		return nil, err
	}
	if _, err := db.Exec("INSERT INTO users(username,password,balance) VALUES($1,$2,$3) ON CONFLICT (username) DO NOTHING", "worker2", pw2, 800); err != nil {
		return nil, err
	}
	return db, nil
}


// retryInitDB calls initDB with retries to handle transient
// PostgreSQL connection failures during container (re)start.
func retryInitDB(dsn string) (*sql.DB, error) {
	var lastErr error
	for attempt := 0; attempt < 5; attempt++ {
		db, err := initDB(dsn)
		if err == nil {
			return db, nil
		}
		lastErr = err
		log.Printf("init db attempt %d failed: %v", attempt+1, err)
		time.Sleep(time.Duration(attempt+1) * time.Second)
	}
	return nil, fmt.Errorf("init db failed after 5 attempts: %w", lastErr)
}

func newUserAuthServer(db *sql.DB, secret string) *userAuthServer {
	return &userAuthServer{db: db, jwtSecret: []byte(secret)}
}

func (u *userAuthServer) issueToken(username string) (string, error) {
	now := time.Now()
	claims := &userClaims{Username: username, RegisteredClaims: jwt.RegisteredClaims{IssuedAt: jwt.NewNumericDate(now), ExpiresAt: jwt.NewNumericDate(now.Add(24 * time.Hour)), Subject: username}}
	t := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	return t.SignedString(u.jwtSecret)
}

func (u *userAuthServer) usernameFromToken(token string) (string, error) {
	parsed, err := jwt.ParseWithClaims(token, &userClaims{}, func(t *jwt.Token) (interface{}, error) { return u.jwtSecret, nil })
	if err != nil || !parsed.Valid {
		return "", fmt.Errorf("invalid token")
	}
	claims, ok := parsed.Claims.(*userClaims)
	if !ok || strings.TrimSpace(claims.Username) == "" {
		return "", fmt.Errorf("invalid token")
	}
	return claims.Username, nil
}

func (u *userAuthServer) Login(ctx context.Context, req *pb.LoginRequest) (*pb.LoginResponse, error) {
	_ = ctx
	username := strings.TrimSpace(req.GetUsername())
	password := req.GetPassword()
	if username == "" || password == "" {
		return &pb.LoginResponse{Success: false, StatusMessage: "username/password required"}, nil
	}

	var pw string
	err := u.db.QueryRow("SELECT password FROM users WHERE username = $1", username).Scan(&pw)
	if err != nil || !verifyPassword(pw, password) {
		return &pb.LoginResponse{Success: false, StatusMessage: "invalid credentials"}, nil
	}
	if !strings.HasPrefix(pw, "$2") {
		if h, hErr := hashPassword(password); hErr == nil {
			_, _ = u.db.Exec("UPDATE users SET password=$1 WHERE username=$2", h, username)
		}
	}
	token, err := u.issueToken(username)
	if err != nil {
		return &pb.LoginResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.LoginResponse{Success: true, StatusMessage: "ok", Token: token}, nil
}

func (u *userAuthServer) registerUser(username, password string) (bool, string) {
	username = strings.TrimSpace(username)
	if username == "" || strings.TrimSpace(password) == "" {
		return false, "username/password required"
	}
	if len(password) < 6 {
		return false, "password too short"
	}
	if u.db == nil {
		return false, "db not initialized"
	}
	hashed, err := hashPassword(password)
	if err != nil {
		return false, err.Error()
	}
	if _, err := u.db.Exec("INSERT INTO users(username,password,balance) VALUES($1,$2,0)", username, hashed); err != nil {
		if strings.Contains(strings.ToLower(err.Error()), "unique") || strings.Contains(strings.ToLower(err.Error()), "constraint") {
			return false, "username already exists"
		}
		return false, err.Error()
	}
	return true, "registered"
}

func (u *userAuthServer) GetBalance(ctx context.Context, req *pb.GetBalanceRequest) (*pb.GetBalanceResponse, error) {
	_ = ctx
	if strings.TrimSpace(req.GetToken()) == "" {
		return &pb.GetBalanceResponse{Success: false, StatusMessage: "token required"}, nil
	}
	username, err := u.usernameFromToken(req.GetToken())
	if err != nil {
		return &pb.GetBalanceResponse{Success: false, StatusMessage: "invalid token"}, nil
	}
	if req.GetUsername() != "" && req.GetUsername() != username {
		return &pb.GetBalanceResponse{Success: false, StatusMessage: "token-user mismatch"}, nil
	}
	var balance int64
	if err := u.db.QueryRow("SELECT balance FROM users WHERE username = $1", username).Scan(&balance); err != nil {
		return &pb.GetBalanceResponse{Success: false, StatusMessage: "user not found"}, nil
	}
	return &pb.GetBalanceResponse{Success: true, StatusMessage: "ok", Balance: balance}, nil
}

func (u *userAuthServer) RefreshToken(ctx context.Context, req *pb.RefreshTokenRequest) (*pb.RefreshTokenResponse, error) {
	_ = ctx
	if strings.TrimSpace(req.GetOldToken()) == "" {
		return &pb.RefreshTokenResponse{Success: false, StatusMessage: "old_token required"}, nil
	}
	username, err := u.usernameFromToken(req.GetOldToken())
	if err != nil {
		return &pb.RefreshTokenResponse{Success: false, StatusMessage: "invalid token"}, nil
	}
	nt, err := u.issueToken(username)
	if err != nil {
		return &pb.RefreshTokenResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.RefreshTokenResponse{Success: true, StatusMessage: "ok", NewToken: nt}, nil
}

// saveTaskToRedis stores task metadata in Redis as a hash
func (m *masterNodeServer) saveTaskToRedis(t *taskState) {
	if m.redis == nil || t == nil {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	key := fmt.Sprintf("task:%s", t.TaskID)
	billingSettled := 0
	if t.BillingSettled {
		billingSettled = 1
	}

	data := map[string]interface{}{
		"task_id":           t.TaskID,
		"owner":             t.Owner,
		"worker_id":         t.WorkerID,
		"worker_ip":         t.WorkerIP,
		"status":            t.Status,
		"status_message":    t.StatusMessage,
		"output":            t.Output,
		"result_torrent":    t.ResultTorrent,
		"torrent_source":    t.TorrentSource,
		"expected_btih":     t.ExpectedBTIH,
		"cpu_usage":         int(t.CpuUsage),
		"memory_usage":      int(t.MemoryUsage),
		"gpu_usage":         int(t.GpuUsage),
		"gpu_memory_usage":  int(t.GpuMemoryUsage),
		"req_cpu_score":     int(t.ReqCPUScore),
		"req_gpu_score":     int(t.ReqGPUScore),
		"req_memory_gb":     int(t.ReqMemoryGB),
		"req_gpu_memory_gb": int(t.ReqGPUMemoryGB),
		"host_count":        int(t.HostCount),
		"billing_settled":   billingSettled,
		"billed_amount":     int(t.BilledAmount),
		"updated_at":        time.Now().Unix(),
		"last_update":       t.LastUpdate.Unix(),
		"retry_count":       int(t.RetryCount),
	}

	if !t.LastSettlementAt.IsZero() {
		data["last_settlement"] = t.LastSettlementAt.Unix()
	}

	err := m.redis.HSet(ctx, key, data).Err()
	if err != nil {
		nodepoolLog(fmt.Sprintf("redis_save_error task_id=%s err=%s", t.TaskID, err.Error()))
		return
	}

	// Update owner's task index
	ownerKey := fmt.Sprintf("tasks:owner:%s", t.Owner)
	m.redis.SAdd(ctx, ownerKey, t.TaskID)

	// Update active tasks index
	if t.Status != "COMPLETED" && t.Status != "FAILED" && t.Status != "STOPPED" {
		m.redis.SAdd(ctx, "tasks:active", t.TaskID)
	} else {
		m.redis.SRem(ctx, "tasks:active", t.TaskID)
		// Set TTL for completed/failed tasks (7 days)
		m.redis.Expire(ctx, key, 7*24*time.Hour)
	}
}

func (m *masterNodeServer) saveTaskLocked(t *taskState) {
	if m.db == nil || t == nil {
		return
	}
	billingSettled := 0
	if t.BillingSettled {
		billingSettled = 1
	}
	_, _ = m.db.Exec(`INSERT INTO tasks(task_id,owner,worker_id,worker_ip,status,status_message,output,result_torrent,torrent_source,expected_btih,cpu_usage,memory_usage,gpu_usage,gpu_memory_usage,req_cpu_score,req_gpu_score,req_memory_gb,req_gpu_memory_gb,host_count,billing_settled,billed_amount,updated_at)
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,NOW())
ON CONFLICT(task_id) DO UPDATE SET owner=EXCLUDED.owner,worker_id=EXCLUDED.worker_id,worker_ip=EXCLUDED.worker_ip,status=EXCLUDED.status,status_message=EXCLUDED.status_message,output=EXCLUDED.output,result_torrent=EXCLUDED.result_torrent,torrent_source=EXCLUDED.torrent_source,expected_btih=EXCLUDED.expected_btih,cpu_usage=EXCLUDED.cpu_usage,memory_usage=EXCLUDED.memory_usage,gpu_usage=EXCLUDED.gpu_usage,gpu_memory_usage=EXCLUDED.gpu_memory_usage,req_cpu_score=EXCLUDED.req_cpu_score,req_gpu_score=EXCLUDED.req_gpu_score,req_memory_gb=EXCLUDED.req_memory_gb,req_gpu_memory_gb=EXCLUDED.req_gpu_memory_gb,host_count=EXCLUDED.host_count,billing_settled=EXCLUDED.billing_settled,billed_amount=EXCLUDED.billed_amount,updated_at=NOW()`,
		t.TaskID, t.Owner, t.WorkerID, t.WorkerIP, t.Status, t.StatusMessage, t.Output, t.ResultTorrent, t.TorrentSource, t.ExpectedBTIH, t.CpuUsage, t.MemoryUsage, t.GpuUsage, t.GpuMemoryUsage, t.ReqCPUScore, t.ReqGPUScore, t.ReqMemoryGB, t.ReqGPUMemoryGB, t.HostCount, billingSettled, t.BilledAmount)

	// Also save to Redis for fast access and horizontal scaling
	m.saveTaskToRedis(t)
}

func (m *masterNodeServer) loadTasksFromDB() {
	if m.db == nil {
		return
	}
	rows, err := m.db.Query(`SELECT task_id,owner,worker_id,worker_ip,status,status_message,output,result_torrent,torrent_source,expected_btih,cpu_usage,memory_usage,gpu_usage,gpu_memory_usage,req_cpu_score,req_gpu_score,req_memory_gb,req_gpu_memory_gb,host_count,billing_settled,billed_amount FROM tasks`)
	if err != nil {
		return
	}
	defer rows.Close()
	for rows.Next() {
		t := &taskState{}
		if err := rows.Scan(&t.TaskID, &t.Owner, &t.WorkerID, &t.WorkerIP, &t.Status, &t.StatusMessage, &t.Output, &t.ResultTorrent, &t.TorrentSource, &t.ExpectedBTIH, &t.CpuUsage, &t.MemoryUsage, &t.GpuUsage, &t.GpuMemoryUsage, &t.ReqCPUScore, &t.ReqGPUScore, &t.ReqMemoryGB, &t.ReqGPUMemoryGB, &t.HostCount, &t.BillingSettled, &t.BilledAmount); err == nil {
			t.LastUpdate = time.Now()
			if t.BilledAmount > 0 {
				t.LastSettlementAt = time.Now()
			}
			m.tasks[t.TaskID] = t
			if t.WorkerIP != "" {
				m.taskToWorker[t.TaskID] = t.WorkerIP
				if m.taskRoutes == nil {
					m.taskRoutes = make(map[string]map[string]string)
				}
				if m.taskRoutes[t.TaskID] == nil {
					m.taskRoutes[t.TaskID] = make(map[string]string)
				}
				m.taskRoutes[t.TaskID][t.WorkerIP] = t.WorkerIP
			}
		}
	}
}

func calcTaskSettlementAmount(t *taskState) int64 {
	if t == nil {
		return 0
	}
	hostCount := int64(t.HostCount)
	if hostCount <= 0 {
		hostCount = 1
	}
	cpuPart := int64(t.ReqCPUScore) / 100
	if int64(t.ReqCPUScore)%100 != 0 {
		cpuPart++
	}
	if cpuPart <= 0 {
		cpuPart = 1
	}
	gpuPart := int64(t.ReqGPUScore)
	if gpuPart < 0 {
		gpuPart = 0
	}
	ramPart := int64(t.ReqMemoryGB)
	if ramPart < 0 {
		ramPart = 0
	}
	gpuMemPart := int64(t.ReqGPUMemoryGB) / 2
	if gpuMemPart < 0 {
		gpuMemPart = 0
	}
	amount := (cpuPart + gpuPart + ramPart + gpuMemPart) * hostCount
	if amount <= 0 {
		amount = 1
	}
	return amount
}

func (m *masterNodeServer) settleTaskPaymentTickLocked(t *taskState) (bool, string) {
	if t == nil {
		return false, "task not found"
	}
	if m.db == nil {
		amount := calcTaskSettlementAmount(t)
		t.BilledAmount += amount
		t.LastSettlementAt = time.Now()
		return true, "settled (no db)"
	}
	if strings.TrimSpace(t.Owner) == "" {
		return true, "settlement skipped: owner missing"
	}
	if strings.TrimSpace(t.WorkerID) == "" {
		return true, "settlement skipped: worker missing"
	}
	if t.Owner == t.WorkerID {
		return true, "settlement skipped: same account"
	}

	amount := calcTaskSettlementAmount(t)
	tx, err := m.db.Begin()
	if err != nil {
		return false, "settlement db begin failed"
	}
	defer tx.Rollback()
	var ownerBalance int64
	if err := tx.QueryRow("SELECT balance FROM users WHERE username = $1", t.Owner).Scan(&ownerBalance); err != nil {
		return false, "payer not found"
	}
	var payeeExists int
	if err := tx.QueryRow("SELECT 1 FROM users WHERE username = $1", t.WorkerID).Scan(&payeeExists); err != nil {
		return true, "settlement skipped: payee not found"
	}
	if ownerBalance < amount {
		return false, "insufficient balance"
	}

	if _, err := tx.Exec("UPDATE users SET balance = balance - $1 WHERE username = $2", amount, t.Owner); err != nil {
		return false, "payer debit failed"
	}
	if _, err := tx.Exec("UPDATE users SET balance = balance + $1 WHERE username = $2", amount, t.WorkerID); err != nil {
		return false, "payee credit failed"
	}
	if _, err := tx.Exec("INSERT INTO cpt_transfers(task_id,payer,payee,amount) VALUES($1,$2,$3,$4)", t.TaskID, t.Owner, t.WorkerID, amount); err != nil {
		return false, "transfer log failed"
	}

	if err := tx.Commit(); err != nil {
		return false, "settlement commit failed"
	}
	t.BilledAmount += amount
	t.LastSettlementAt = time.Now()
	return true, "settled"
}

func extractBTIHFromSource(src string) (string, error) {
	src = strings.TrimSpace(src)
	if src == "" {
		return "", fmt.Errorf("empty source")
	}
	if strings.HasPrefix(strings.ToLower(src), "magnet:?") {
		u, err := url.Parse(src)
		if err != nil {
			return "", err
		}
		xt := strings.ToLower(strings.TrimSpace(u.Query().Get("xt")))
		if !strings.HasPrefix(xt, "urn:btih:") {
			return "", fmt.Errorf("missing xt=urn:btih")
		}
		h := strings.TrimPrefix(xt, "urn:btih:")
		if len(h) != 40 {
			s := sha1.Sum([]byte(src))
			return hex.EncodeToString(s[:]), nil
		}
		if _, err := hex.DecodeString(h); err != nil {
			s := sha1.Sum([]byte(src))
			return hex.EncodeToString(s[:]), nil
		}
		return h, nil
	}
	if strings.HasPrefix(strings.ToLower(src), "http://") || strings.HasPrefix(strings.ToLower(src), "https://") {
		u, err := url.Parse(src)
		if err != nil {
			return "", err
		}
		h := strings.ToLower(strings.TrimSpace(u.Query().Get("ih")))
		if h == "" {
			s := sha1.Sum([]byte(src))
			return hex.EncodeToString(s[:]), nil
		}
		if len(h) != 40 {
			s := sha1.Sum([]byte(src))
			return hex.EncodeToString(s[:]), nil
		}
		if _, err := hex.DecodeString(h); err != nil {
			s := sha1.Sum([]byte(src))
			return hex.EncodeToString(s[:]), nil
		}
		return h, nil
	}
	s := sha1.Sum([]byte(src))
	return hex.EncodeToString(s[:]), nil
}

func extractStrictBTIHFromSource(src string) (string, error) {
	src = strings.TrimSpace(src)
	if src == "" {
		return "", fmt.Errorf("empty source")
	}
	if strings.HasPrefix(strings.ToLower(src), "magnet:?") {
		u, err := url.Parse(src)
		if err != nil {
			return "", err
		}
		xt := strings.ToLower(strings.TrimSpace(u.Query().Get("xt")))
		if !strings.HasPrefix(xt, "urn:btih:") {
			return "", fmt.Errorf("missing xt=urn:btih")
		}
		h := strings.TrimPrefix(xt, "urn:btih:")
		if len(h) != 40 {
			return "", fmt.Errorf("invalid btih length")
		}
		if _, err := hex.DecodeString(h); err != nil {
			return "", fmt.Errorf("invalid btih hex")
		}
		return h, nil
	}
	if strings.HasPrefix(strings.ToLower(src), "http://") || strings.HasPrefix(strings.ToLower(src), "https://") {
		u, err := url.Parse(src)
		if err != nil {
			return "", err
		}
		h := strings.ToLower(strings.TrimSpace(u.Query().Get("ih")))
		if h == "" {
			return "", fmt.Errorf("missing ih")
		}
		if len(h) != 40 {
			return "", fmt.Errorf("invalid ih length")
		}
		if _, err := hex.DecodeString(h); err != nil {
			return "", fmt.Errorf("invalid ih hex")
		}
		return h, nil
	}
	return "", fmt.Errorf("unsupported source for strict btih")
}

func extractBTIHFromResult(result string) string {
	u, err := url.Parse(strings.TrimSpace(result))
	if err != nil {
		return ""
	}
	h := strings.ToLower(strings.TrimSpace(u.Query().Get("btih")))
	if len(h) == 40 {
		if _, err := hex.DecodeString(h); err == nil {
			return h
		}
	}
	if strings.HasPrefix(strings.ToLower(result), "magnet:?") {
		xt := strings.ToLower(strings.TrimSpace(u.Query().Get("xt")))
		if strings.HasPrefix(xt, "urn:btih:") {
			h2 := strings.TrimPrefix(xt, "urn:btih:")
			if len(h2) == 40 {
				if _, err := hex.DecodeString(h2); err == nil {
					return h2
				}
			}
		}
	}
	return ""
}

func (m *masterNodeServer) validateTokenFromMasterRequest(token string) (string, error) {
	if strings.TrimSpace(token) == "" {
		return "", fmt.Errorf("token required")
	}
	return m.auth.usernameFromToken(token)
}

func (m *masterNodeServer) validateWorkerIngressToken(taskID, token string) (string, error) {
	if !envBool("NODEPOOL_WORKER_INGRESS_AUTH", true) {
		return "", nil
	}
	if strings.TrimSpace(taskID) == "" {
		return "", fmt.Errorf("task_id is required")
	}
	if strings.TrimSpace(token) == "" {
		return "", fmt.Errorf("token required")
	}
	username, err := m.auth.usernameFromToken(token)
	if err != nil {
		return "", fmt.Errorf("invalid token")
	}
	t, ok := m.getTask(taskID)
	if !ok || t == nil {
		return "", fmt.Errorf("task not found")
	}
	if strings.TrimSpace(t.WorkerID) != "" && t.WorkerID != username {
		return "", fmt.Errorf("token-worker mismatch")
	}
	return username, nil
}

func (m *masterNodeServer) setTaskRoute(taskID, workerAddr string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.taskToWorker[taskID] = workerAddr
	if m.taskRoutes == nil {
		m.taskRoutes = make(map[string]map[string]string)
	}
	if m.taskRoutes[taskID] == nil {
		m.taskRoutes[taskID] = make(map[string]string)
	}
	m.taskRoutes[taskID][workerAddr] = workerAddr
}

func (m *masterNodeServer) getTaskRoute(taskID string) (string, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	if routes, ok := m.taskRoutes[taskID]; ok {
		for _, addr := range routes {
			if strings.TrimSpace(addr) != "" {
				return addr, true
			}
		}
	}
	addr, ok := m.taskToWorker[taskID]
	return addr, ok
}

func (m *masterNodeServer) getTaskRoutes(taskID string) []string {
	m.mu.RLock()
	defer m.mu.RUnlock()
	out := make([]string, 0)
	seen := make(map[string]bool)
	if routes, ok := m.taskRoutes[taskID]; ok {
		for _, addr := range routes {
			addr = strings.TrimSpace(addr)
			if addr == "" || seen[addr] {
				continue
			}
			seen[addr] = true
			out = append(out, addr)
		}
	}
	if addr, ok := m.taskToWorker[taskID]; ok {
		addr = strings.TrimSpace(addr)
		if addr != "" && !seen[addr] {
			out = append(out, addr)
		}
	}
	return out
}

func (m *masterNodeServer) deleteTaskRoute(taskID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.taskToWorker, taskID)
	delete(m.taskRoutes, taskID)
}

func (m *masterNodeServer) deleteTaskRouteAddr(taskID, addr string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	addr = strings.TrimSpace(addr)
	if addr == "" {
		return
	}
	if cur, ok := m.taskToWorker[taskID]; ok && cur == addr {
		delete(m.taskToWorker, taskID)
	}
	if routes, ok := m.taskRoutes[taskID]; ok {
		delete(routes, addr)
		if len(routes) == 0 {
			delete(m.taskRoutes, taskID)
		}
	}
}

func (m *masterNodeServer) upsertTask(taskID, owner, workerID, workerIP, status, statusMessage string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.tasks[taskID] == nil {
		m.tasks[taskID] = &taskState{TaskID: taskID}
	}
	t := m.tasks[taskID]
	if owner != "" {
		t.Owner = owner
	}
	if workerID != "" {
		t.WorkerID = workerID
	}
	t.WorkerIP = workerIP
	t.Status = status
	t.StatusMessage = statusMessage
	t.LastUpdate = time.Now()
	m.saveTaskLocked(t)
}

func appendTaskLogLocked(t *taskState, message string) {
	if t == nil {
		return
	}
	message = strings.TrimSpace(message)
	if message == "" {
		return
	}
	if strings.TrimSpace(t.Output) == "" {
		t.Output = message
		return
	}
	t.Output = t.Output + "\n" + message
}

func (m *masterNodeServer) addTaskSystemLog(taskID, message string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	t, ok := m.tasks[taskID]
	if !ok || t == nil {
		return
	}
	appendTaskLogLocked(t, message)
	t.LastUpdate = time.Now()
	m.saveTaskLocked(t)
}

func (m *masterNodeServer) getTask(taskID string) (*taskState, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	t, ok := m.tasks[taskID]
	if !ok || t == nil {
		return nil, false
	}
	cp := *t
	return &cp, true
}

func (m *masterNodeServer) setTaskResult(taskID, resultTorrent string) (bool, string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	t, ok := m.tasks[taskID]
	if !ok || t == nil {
		return false, "task not found"
	}
	if t.ExpectedBTIH != "" {
		got := extractBTIHFromResult(resultTorrent)
		if m.strictBTIH && got == "" {
			t.Status = "FAILED"
			t.StatusMessage = "result missing btih"
			m.saveTaskLocked(t)
			return false, "result missing btih"
		}
		if got != "" && got != t.ExpectedBTIH {
			t.Status = "FAILED"
			t.StatusMessage = "btih mismatch"
			m.saveTaskLocked(t)
			return false, "btih mismatch"
		}
	}
	t.ResultTorrent = resultTorrent
	t.Status = "COMPLETED"
	t.StatusMessage = "result uploaded"
	t.LastUpdate = time.Now()
	if t.WorkerIP == "" {
		if addr, ok := m.taskToWorker[taskID]; ok {
			t.WorkerIP = addr
		}
	}
	if !t.BillingSettled && t.BilledAmount == 0 {
		if ok, msg := m.settleTaskPaymentTickLocked(t); !ok {
			t.Status = "FAILED"
			t.StatusMessage = msg
			delete(m.taskToWorker, taskID)
			delete(m.taskRoutes, taskID)
			m.saveTaskLocked(t)
			return false, msg
		} else if msg == "settled" || msg == "settled (no db)" {
			t.StatusMessage = fmt.Sprintf("result uploaded, settled %d CPT", t.BilledAmount)
		}
	}
	t.BillingSettled = true
	m.saveTaskLocked(t)
	return true, "uploaded"
}

func (m *masterNodeServer) setTaskOutput(taskID, output string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	t, ok := m.tasks[taskID]
	if !ok || t == nil {
		return false
	}
	appendTaskLogLocked(t, output)

	// 記錄任務輸出到日誌文件
	outputLen := len(output)
	if outputLen <= 500 {
		// 短輸出：完整記錄
		log.Printf("task_output_received task_id=%s worker_id=%s output=%s", taskID, t.WorkerID, output)
	} else {
		// 長輸出：記錄摘要
		preview := output
		if outputLen > 200 {
			preview = output[:200] + "..."
		}
		log.Printf("task_output_received task_id=%s worker_id=%s length=%d preview=%s", taskID, t.WorkerID, outputLen, preview)
	}

	lower := strings.ToLower(strings.TrimSpace(output))
	if strings.HasPrefix(lower, "task failed") || strings.HasPrefix(lower, "program error") {
		t.Status = "FAILED"
		t.StatusMessage = output
		delete(m.taskToWorker, taskID)
		delete(m.taskRoutes, taskID)
		t.LastUpdate = time.Now()
		m.saveTaskLocked(t)
		log.Printf("task_failed task_id=%s reason=%s", taskID, output)
		return true
	}
	if t.Status == "" || t.Status == "DISPATCHED" || t.Status == "PENDING" {
		t.Status = "RUNNING"
		t.StatusMessage = "output uploaded"
	}
	t.LastUpdate = time.Now()
	m.saveTaskLocked(t)
	return true
}

func (m *masterNodeServer) setTaskUsage(taskID string, cpuUsage, memoryUsage, gpuUsage, gpuMemoryUsage float32) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	t, ok := m.tasks[taskID]
	if !ok || t == nil {
		return false
	}
	t.CpuUsage = cpuUsage
	t.MemoryUsage = memoryUsage
	t.GpuUsage = gpuUsage
	t.GpuMemoryUsage = gpuMemoryUsage
	if t.Status == "" || t.Status == "DISPATCHED" || t.Status == "PENDING" {
		t.Status = "RUNNING"
		t.StatusMessage = "usage uploaded"
	}
	t.LastUpdate = time.Now()
	m.saveTaskLocked(t)
	return true
}

func (m *masterNodeServer) listTasksByOwner(owner string) []*taskState {
	m.mu.RLock()
	defer m.mu.RUnlock()
	out := make([]*taskState, 0)
	for _, t := range m.tasks {
		if t == nil || t.Owner != owner {
			continue
		}
		cp := *t
		out = append(out, &cp)
	}
	return out
}

func (m *masterNodeServer) listTasksByOwnerFiltered(owner, status, keyword string, fromTS, toTS int64, limit, offset int, sortBy string, asc bool) ([]*taskState, int) {
	all := m.listTasksByOwner(owner)
	status = strings.TrimSpace(strings.ToUpper(status))
	keyword = strings.ToLower(strings.TrimSpace(keyword))

	filtered := make([]*taskState, 0, len(all))
	for _, t := range all {
		if t == nil {
			continue
		}
		if status != "" && strings.ToUpper(strings.TrimSpace(t.Status)) != status {
			continue
		}
		if keyword != "" {
			blob := strings.ToLower(t.TaskID + " " + t.StatusMessage + " " + t.WorkerID + " " + t.WorkerIP)
			if !strings.Contains(blob, keyword) {
				continue
			}
		}
		if fromTS > 0 && t.LastUpdate.Unix() < fromTS {
			continue
		}
		if toTS > 0 && t.LastUpdate.Unix() > toTS {
			continue
		}
		filtered = append(filtered, t)
	}

	sortBy = strings.TrimSpace(strings.ToLower(sortBy))
	if sortBy == "" {
		sortBy = "updated_at"
	}
	sort.SliceStable(filtered, func(i, j int) bool {
		a := filtered[i]
		b := filtered[j]
		var less bool
		switch sortBy {
		case "task_id":
			less = a.TaskID < b.TaskID
		case "status":
			less = a.Status < b.Status
		default:
			less = a.LastUpdate.Before(b.LastUpdate)
		}
		if asc {
			return less
		}
		return !less
	})

	total := len(filtered)
	if offset < 0 {
		offset = 0
	}
	if limit <= 0 {
		limit = 50
	}
	if limit > 500 {
		limit = 500
	}
	if offset >= total {
		return []*taskState{}, total
	}
	end := offset + limit
	if end > total {
		end = total
	}
	return filtered[offset:end], total
}

func (m *masterNodeServer) dispatchTaskToWorkerWithExcludes(ctx context.Context, t *taskState, excludes map[string]bool) (workerID, workerAddr string, reason string, ok bool) {
	probeEnabled := envBool("NODEPOOL_PRE_DISPATCH_PROBE", true)
	workers, err := m.svc.ListAvailableWorkers(ctx)
	if err != nil || len(workers) == 0 {
		if err != nil {
			return "", "", fmt.Sprintf("list workers failed: %v", err), false
		}
		if !probeEnabled {
			allWorkers, listErr := m.svc.ListWorkers(ctx, true)
			if listErr != nil || len(allWorkers) == 0 {
				if listErr != nil {
					return "", "", fmt.Sprintf("list workers failed: %v", listErr), false
				}
				return "", "", "no available worker", false
			}
			workers = allWorkers
		} else {
			return "", "", "no available worker", false
		}
	}
	probeTimeout := envDurationSeconds("NODEPOOL_WORKER_PROBE_TIMEOUT_SEC", 5*time.Second)
	lastReason := "no available worker"
	healthyWorkers := make([]*repository.Worker, 0, len(workers))
	for _, w := range workers {
		if w == nil || strings.TrimSpace(w.Addr) == "" {
			lastReason = "worker address missing"
			continue
		}
		if excludes != nil && excludes[w.Addr] {
			lastReason = "worker excluded"
			continue
		}
		if !probeEnabled {
			healthyWorkers = append(healthyWorkers, w)
			continue
		}
		dialCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
		conn, dialErr := grpc.DialContext(dialCtx, w.Addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
		cancel()
		if dialErr != nil {
			lastReason = fmt.Sprintf("dial worker %s failed", w.Addr)
			_ = m.svc.MarkWorkerOffline(ctx, w.ID)
			continue
		}
		client := pb.NewWorkerNodeServiceClient(conn)
		if probeEnabled {
			probeCtx, probeCancel := context.WithTimeout(ctx, probeTimeout)
			_, probeErr := client.TaskOutput(probeCtx, &pb.TaskOutputRequest{TaskId: "__hivemind_probe__"})
			probeCancel()
			if probeErr != nil {
				lastReason = fmt.Sprintf("worker probe failed at %s", w.Addr)
				_ = m.svc.MarkWorkerOffline(ctx, w.ID)
				_ = conn.Close()
				continue
			}
		}
		healthyWorkers = append(healthyWorkers, w)
		_ = conn.Close()
	}
	if len(healthyWorkers) == 0 {
		return "", "", lastReason, false
	}
	for _, w := range healthyWorkers {
		dialCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
		conn, dialErr := grpc.DialContext(dialCtx, w.Addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
		cancel()
		if dialErr != nil {
			lastReason = fmt.Sprintf("dial worker %s failed", w.Addr)
			_ = m.svc.MarkWorkerOffline(ctx, w.ID)
			continue
		}
		client := pb.NewWorkerNodeServiceClient(conn)
		execCtx, execCancel := context.WithTimeout(ctx, 3*time.Second)
		resp, execErr := client.ExecuteTask(execCtx, &pb.ExecuteTaskRequest{TaskId: t.TaskID, Torrent: t.TorrentSource, CpuUsage: 0, GpuUsage: 0, MemoryGb: t.ReqMemoryGB, GpuMemoryGb: t.ReqGPUMemoryGB})
		execCancel()
		_ = conn.Close()
		if execErr != nil {
			lastReason = fmt.Sprintf("execute rpc failed at %s", w.Addr)
			_ = m.svc.MarkWorkerOffline(ctx, w.ID)
			continue
		}
		if resp.GetSuccess() {
			return w.ID, w.Addr, "", true
		}
		lastReason = fmt.Sprintf("worker rejected task at %s", w.Addr)
	}
	return "", "", lastReason, false
}

func (m *masterNodeServer) dispatchTaskToWorker(ctx context.Context, t *taskState, excludeAddr string) (workerID, workerAddr string, reason string, ok bool) {
	excludes := map[string]bool{}
	if strings.TrimSpace(excludeAddr) != "" {
		excludes[excludeAddr] = true
	}
	return m.dispatchTaskToWorkerWithExcludes(ctx, t, excludes)
}

func (m *masterNodeServer) processTaskTimeouts(taskTimeout time.Duration, maxRedispatch int) {
	if taskTimeout <= 0 {
		return
	}
	if maxRedispatch < 0 {
		maxRedispatch = 0
	}

	now := time.Now()
	timedOut := make([]string, 0)

	m.mu.RLock()
	for id, t := range m.tasks {
		if t == nil {
			continue
		}
		if t.ResultTorrent != "" {
			continue
		}
		if t.Status != "DISPATCHED" && t.Status != "RUNNING" {
			continue
		}
		if t.LastUpdate.IsZero() {
			continue
		}
		if now.Sub(t.LastUpdate) >= taskTimeout {
			timedOut = append(timedOut, id)
		}
	}
	m.mu.RUnlock()

	for _, taskID := range timedOut {
		var snapshot taskState
		var oldAddr string

		m.mu.Lock()
		t, ok := m.tasks[taskID]
		if !ok || t == nil {
			m.mu.Unlock()
			continue
		}
		if t.ResultTorrent != "" || (t.Status != "DISPATCHED" && t.Status != "RUNNING") {
			m.mu.Unlock()
			continue
		}
		t.RetryCount++
		t.LastUpdate = time.Now()
		oldAddr = t.WorkerIP
		delete(m.taskToWorker, taskID)
		delete(m.taskRoutes, taskID)
		if int(t.RetryCount) > maxRedispatch {
			t.Status = "FAILED"
			t.StatusMessage = "worker timeout, retries exhausted"
			m.saveTaskLocked(t)
			nodepoolLog(fmt.Sprintf("task_timeout_failed task_id=%s worker_id=%s worker_addr=%s retry=%d/%d", taskID, t.WorkerID, oldAddr, t.RetryCount, maxRedispatch))
			m.mu.Unlock()
			continue
		}
		t.Status = "PENDING"
		t.StatusMessage = "worker timeout, redispatching"
		m.saveTaskLocked(t)
		nodepoolLog(fmt.Sprintf("task_timeout_redispatch task_id=%s worker_id=%s worker_addr=%s retry=%d/%d", taskID, t.WorkerID, oldAddr, t.RetryCount, maxRedispatch))
		snapshot = *t
		m.mu.Unlock()

		workerID, workerAddr, reason, ok := m.dispatchTaskToWorker(context.Background(), &snapshot, oldAddr)

		m.mu.Lock()
		t2, ok2 := m.tasks[taskID]
		if !ok2 || t2 == nil {
			m.mu.Unlock()
			continue
		}
		if t2.ResultTorrent != "" || t2.Status == "FAILED" || t2.Status == "STOPPED" {
			m.mu.Unlock()
			continue
		}
		if ok {
			t2.WorkerID = workerID
			t2.WorkerIP = workerAddr
			t2.Status = "DISPATCHED"
			t2.StatusMessage = fmt.Sprintf("task redispatched (%d/%d)", t2.RetryCount, maxRedispatch)
			t2.LastUpdate = time.Now()
			m.taskToWorker[taskID] = workerAddr
			if m.taskRoutes == nil {
				m.taskRoutes = make(map[string]map[string]string)
			}
			if m.taskRoutes[taskID] == nil {
				m.taskRoutes[taskID] = make(map[string]string)
			}
			m.taskRoutes[taskID][workerAddr] = workerAddr
			m.saveTaskLocked(t2)
			nodepoolLog(fmt.Sprintf("redispatch_success task_id=%s worker_id=%s worker_addr=%s retry=%d/%d", taskID, workerID, workerAddr, t2.RetryCount, maxRedispatch))
		} else {
			t2.Status = "PENDING"
			t2.StatusMessage = fmt.Sprintf("redispatch waiting worker (%d/%d): %s", t2.RetryCount, maxRedispatch, formatDispatchReason(reason))
			appendTaskLogLocked(t2, t2.StatusMessage)
			t2.LastUpdate = time.Now()
			m.saveTaskLocked(t2)
			nodepoolLog(fmt.Sprintf("redispatch_waiting task_id=%s retry=%d/%d reason=%s", taskID, t2.RetryCount, maxRedispatch, formatDispatchReason(reason)))
		}
		m.mu.Unlock()
	}
}

func (m *masterNodeServer) startTaskTimeoutMonitor(ctx context.Context, taskTimeout time.Duration, maxRedispatch int) {
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			m.processTaskTimeouts(taskTimeout, maxRedispatch)
		}
	}
}

func (m *masterNodeServer) stopTaskExecutionBestEffort(taskID, workerAddr string) {
	if strings.TrimSpace(taskID) == "" || strings.TrimSpace(workerAddr) == "" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	conn, err := grpc.DialContext(ctx, workerAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Printf("stop task best-effort dial failed task=%s worker=%s err=%v", taskID, workerAddr, err)
		return
	}
	defer conn.Close()
	client := pb.NewWorkerNodeServiceClient(conn)
	resp, err := client.StopTaskExecution(ctx, &pb.StopTaskExecutionRequest{TaskId: taskID})
	if err != nil {
		log.Printf("stop task best-effort rpc failed task=%s worker=%s err=%v", taskID, workerAddr, err)
		return
	}
	if !resp.GetSuccess() {
		log.Printf("stop task best-effort rejected task=%s worker=%s msg=%s", taskID, workerAddr, resp.GetStatusMessage())
	}
}

func (m *masterNodeServer) processPeriodicSettlements(settlementInterval time.Duration) {
	if settlementInterval <= 0 {
		return
	}
	now := time.Now()
	type stopReq struct {
		taskID string
		addr   string
	}
	stopList := make([]stopReq, 0)

	m.mu.Lock()
	defer m.mu.Unlock()

	for _, t := range m.tasks {
		if t == nil {
			continue
		}
		if t.BillingSettled {
			continue
		}
		// Only settle while task is truly running.
		// Settling/refreshing DISPATCHED tasks can keep them alive forever and block timeout redispatch.
		if t.Status != "RUNNING" {
			continue
		}
		if !t.LastSettlementAt.IsZero() && now.Sub(t.LastSettlementAt) < settlementInterval {
			continue
		}

		if ok, msg := m.settleTaskPaymentTickLocked(t); !ok {
			if msg == "insufficient balance" {
				routeSet := make(map[string]bool)
				if addr := strings.TrimSpace(t.WorkerIP); addr != "" {
					routeSet[addr] = true
				}
				if addr, ok := m.taskToWorker[t.TaskID]; ok {
					addr = strings.TrimSpace(addr)
					if addr != "" {
						routeSet[addr] = true
					}
				}
				if routes, ok := m.taskRoutes[t.TaskID]; ok {
					for _, addr := range routes {
						addr = strings.TrimSpace(addr)
						if addr != "" {
							routeSet[addr] = true
						}
					}
				}
				t.Status = "FAILED"
				t.StatusMessage = msg
				t.BillingSettled = true
				delete(m.taskToWorker, t.TaskID)
				delete(m.taskRoutes, t.TaskID)
				t.LastUpdate = now
				m.saveTaskLocked(t)
				for addr := range routeSet {
					stopList = append(stopList, stopReq{taskID: t.TaskID, addr: addr})
				}
			}
			continue
		}

		t.StatusMessage = fmt.Sprintf("running, settled %d CPT", t.BilledAmount)
		m.saveTaskLocked(t)
	}

	// network calls should happen outside lock
	m.mu.Unlock()
	for _, s := range stopList {
		m.stopTaskExecutionBestEffort(s.taskID, s.addr)
	}
	m.mu.Lock()
}

func (m *masterNodeServer) startPeriodicSettlementMonitor(ctx context.Context, settlementInterval time.Duration) {
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			m.processPeriodicSettlements(settlementInterval)
		}
	}
}

func (n *nodeManagerServer) RegisterWorkerNode(ctx context.Context, req *pb.RegisterWorkerNodeRequest) (*pb.StatusResponse, error) {
	w := &repository.Worker{ID: req.Username, Addr: req.Ip, Meta: map[string]string{"cpu_cores": fmt.Sprintf("%d", req.CpuCores), "memory_gb": fmt.Sprintf("%d", req.MemoryGb), "cpu_score": fmt.Sprintf("%d", req.CpuScore), "gpu_score": fmt.Sprintf("%d", req.GpuScore), "gpu_memory": fmt.Sprintf("%d", req.GpuMemoryGb), "location": req.Location}}
	if err := n.svc.RegisterWorker(ctx, w); err != nil {
		nodepoolLog(fmt.Sprintf("register worker username=%s ip=%s cpu_cores=%d memory_gb=%d cpu_score=%d gpu_score=%d gpu_memory=%d location=%s result=failed msg=%s", req.Username, req.Ip, req.CpuCores, req.MemoryGb, req.CpuScore, req.GpuScore, req.GpuMemoryGb, req.Location, err.Error()))
		return &pb.StatusResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	nodepoolLog(fmt.Sprintf("register worker username=%s ip=%s cpu_cores=%d memory_gb=%d cpu_score=%d gpu_score=%d gpu_memory=%d location=%s result=registered", req.Username, req.Ip, req.CpuCores, req.MemoryGb, req.CpuScore, req.GpuScore, req.GpuMemoryGb, req.Location))
	return &pb.StatusResponse{Success: true, StatusMessage: "registered"}, nil
}

func (n *nodeManagerServer) ReportStatus(ctx context.Context, req *pb.RunningStatusRequest) (*pb.RunningStatusResponse, error) {
	if err := n.svc.Heartbeat(ctx, req.Username); err != nil {
		return &pb.RunningStatusResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	return &pb.RunningStatusResponse{Success: true, StatusMessage: "ok"}, nil
}

func (m *masterNodeServer) UploadTask(ctx context.Context, req *pb.UploadTaskRequest) (*pb.UploadTaskResponse, error) {
	owner, err := m.validateTokenFromMasterRequest(req.GetToken())
	if err != nil {
		return &pb.UploadTaskResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	var expectedHash string
	if m.strictBTIH {
		h, err := extractStrictBTIHFromSource(req.GetTorrent())
		if err != nil {
			return &pb.UploadTaskResponse{Success: false, StatusMessage: "invalid torrent source btih"}, nil
		}
		expectedHash = h
	} else {
		expectedHash, _ = extractBTIHFromSource(req.GetTorrent())
	}
	m.mu.Lock()
	if existing := m.tasks[req.GetTaskId()]; existing != nil && strings.TrimSpace(existing.Owner) != "" {
		existingOwner := existing.Owner
		existingStatus := existing.Status
		m.mu.Unlock()
		nodepoolLog(fmt.Sprintf("task_duplicate_rejected task_id=%s owner=%s existing_owner=%s status=%s", req.GetTaskId(), owner, existingOwner, existingStatus))
		return &pb.UploadTaskResponse{Success: false, StatusMessage: "duplicate task_id"}, nil
	}
	if m.tasks[req.GetTaskId()] == nil {
		m.tasks[req.GetTaskId()] = &taskState{TaskID: req.GetTaskId()}
	}
	m.tasks[req.GetTaskId()].Owner = owner
	m.tasks[req.GetTaskId()].TorrentSource = req.GetTorrent()
	m.tasks[req.GetTaskId()].ExpectedBTIH = expectedHash
	m.tasks[req.GetTaskId()].ReqCPUScore = req.GetCpuScore()
	m.tasks[req.GetTaskId()].ReqGPUScore = req.GetGpuScore()
	m.tasks[req.GetTaskId()].ReqMemoryGB = req.GetMemoryGb()
	m.tasks[req.GetTaskId()].ReqGPUMemoryGB = req.GetGpuMemoryGb()
	m.tasks[req.GetTaskId()].HostCount = req.GetHostCount()
	m.tasks[req.GetTaskId()].BillingSettled = false
	m.tasks[req.GetTaskId()].BilledAmount = 0
	m.tasks[req.GetTaskId()].LastUpdate = time.Now()
	m.saveTaskLocked(m.tasks[req.GetTaskId()])
	m.mu.Unlock()
	t, _ := m.getTask(req.GetTaskId())
	if t == nil {
		return &pb.UploadTaskResponse{Success: false, StatusMessage: "task init failed"}, nil
	}
	hostCount := req.GetHostCount()
	if hostCount <= 0 {
		hostCount = 1
	}
	excludes := map[string]bool{}
	workerID, workerAddr, reason, ok := m.dispatchTaskToWorkerWithExcludes(ctx, t, excludes)
	if !ok {
		formattedReason := formatDispatchReason(reason)
		m.upsertTask(req.GetTaskId(), owner, "", "", "PENDING", formattedReason)
		m.addTaskSystemLog(req.GetTaskId(), formattedReason)
		nodepoolLog(fmt.Sprintf("task_dispatch_failed task_id=%s reason=%s", req.GetTaskId(), formattedReason))
		return &pb.UploadTaskResponse{Success: false, StatusMessage: formattedReason}, nil
	}
	nodepoolLog(fmt.Sprintf("task_dispatch_success task_id=%s worker_id=%s worker_addr=%s", req.GetTaskId(), workerID, workerAddr))
	excludes[workerAddr] = true
	m.setTaskRoute(req.GetTaskId(), workerAddr)
	m.upsertTask(req.GetTaskId(), owner, workerID, workerAddr, "DISPATCHED", "task dispatched to worker")
	if hostCount > 1 {
		successCount := int32(1)
		for i := int32(2); i <= hostCount; i++ {
			extraID, extraAddr, extraReason, extraOK := m.dispatchTaskToWorkerWithExcludes(ctx, t, excludes)
			if !extraOK {
				m.addTaskSystemLog(req.GetTaskId(), fmt.Sprintf("[HOST_FANOUT] partial dispatch (%d/%d): %s", successCount, hostCount, formatDispatchReason(extraReason)))
				break
			}
			excludes[extraAddr] = true
			m.setTaskRoute(req.GetTaskId(), extraAddr)
			successCount++
			m.addTaskSystemLog(req.GetTaskId(), fmt.Sprintf("[HOST_FANOUT] dispatched extra worker %s at %s (%d/%d)", extraID, extraAddr, successCount, hostCount))
		}
		if successCount < hostCount {
			m.upsertTask(req.GetTaskId(), owner, workerID, workerAddr, "DISPATCHED", fmt.Sprintf("task dispatched partially (%d/%d workers)", successCount, hostCount))
		} else {
			m.upsertTask(req.GetTaskId(), owner, workerID, workerAddr, "DISPATCHED", fmt.Sprintf("task dispatched to %d workers", hostCount))
		}
	}
	return &pb.UploadTaskResponse{Success: true, StatusMessage: "dispatched"}, nil
}

func (m *masterNodeServer) StopTask(ctx context.Context, req *pb.StopTaskRequest) (*pb.StopTaskResponse, error) {
	owner, err := m.validateTokenFromMasterRequest(req.GetToken())
	if err != nil {
		return &pb.StopTaskResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if req.GetTaskId() == "" {
		return &pb.StopTaskResponse{Success: false, StatusMessage: "task_id is required"}, nil
	}
	t, ok := m.getTask(req.GetTaskId())
	if !ok || t.Owner != owner {
		return &pb.StopTaskResponse{Success: false, StatusMessage: "task not found"}, nil
	}
	routes := m.getTaskRoutes(req.GetTaskId())
	if len(routes) == 0 {
		return &pb.StopTaskResponse{Success: false, StatusMessage: "task route not found"}, nil
	}
	failed := make([]string, 0)
	stopped := 0
	for _, addr := range routes {
		dialCtx, cancel := context.WithTimeout(ctx, 2*time.Second)
		conn, err := grpc.DialContext(dialCtx, addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
		cancel()
		if err != nil {
			failed = append(failed, fmt.Sprintf("%s: dial failed", addr))
			continue
		}
		client := pb.NewWorkerNodeServiceClient(conn)
		stopCtx, stopCancel := context.WithTimeout(ctx, 3*time.Second)
		resp, err := client.StopTaskExecution(stopCtx, &pb.StopTaskExecutionRequest{TaskId: req.GetTaskId()})
		stopCancel()
		_ = conn.Close()
		if err != nil {
			failed = append(failed, fmt.Sprintf("%s: stop rpc failed", addr))
			continue
		}
		if !resp.GetSuccess() {
			failed = append(failed, fmt.Sprintf("%s: %s", addr, resp.GetStatusMessage()))
			continue
		}
		stopped++
		m.deleteTaskRouteAddr(req.GetTaskId(), addr)
	}
	if stopped == 0 {
		m.upsertTask(req.GetTaskId(), owner, t.WorkerID, t.WorkerIP, "STOP_FAILED", strings.Join(failed, "; "))
		return &pb.StopTaskResponse{Success: false, StatusMessage: "failed to stop task on all workers"}, nil
	}
	if len(failed) > 0 {
		m.upsertTask(req.GetTaskId(), owner, t.WorkerID, t.WorkerIP, "STOPPED", fmt.Sprintf("stopped on %d workers, %d failed", stopped, len(failed)))
		return &pb.StopTaskResponse{Success: true, StatusMessage: fmt.Sprintf("partial stop: %d/%d", stopped, len(routes))}, nil
	}
	m.deleteTaskRoute(req.GetTaskId())
	m.upsertTask(req.GetTaskId(), owner, t.WorkerID, t.WorkerIP, "STOPPED", "stopped")
	return &pb.StopTaskResponse{Success: true, StatusMessage: "stopped"}, nil
}

func (m *masterNodeServer) GetAllUserTasks(ctx context.Context, req *pb.GetAllUserTasksRequest) (*pb.GetAllUserTasksResponse, error) {
	_ = ctx
	owner, err := m.validateTokenFromMasterRequest(req.GetToken())
	if err != nil {
		return &pb.GetAllUserTasksResponse{Tasks: []*pb.TaskInfo{}}, nil
	}
	states := m.listTasksByOwner(owner)
	resp := &pb.GetAllUserTasksResponse{Tasks: make([]*pb.TaskInfo, 0, len(states))}
	for _, s := range states {
		resp.Tasks = append(resp.Tasks, &pb.TaskInfo{TaskId: s.TaskID, Status: s.Status, StatusMessage: s.StatusMessage, CpuUsage: s.CpuUsage, MemoryUsage: s.MemoryUsage, GpuUsage: s.GpuUsage, GpuMemoryUsage: s.GpuMemoryUsage, WorkerIp: s.WorkerIP})
	}
	return resp, nil
}

func (m *masterNodeServer) GetTaskResult(ctx context.Context, req *pb.GetTaskResultRequest) (*pb.GetTaskResultResponse, error) {
	_ = ctx
	owner, err := m.validateTokenFromMasterRequest(req.GetToken())
	if err != nil {
		return &pb.GetTaskResultResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if req.GetTaskId() == "" {
		return &pb.GetTaskResultResponse{Success: false, StatusMessage: "task_id is required"}, nil
	}
	t, ok := m.getTask(req.GetTaskId())
	if !ok || t.Owner != owner {
		return &pb.GetTaskResultResponse{Success: false, StatusMessage: "task not found"}, nil
	}
	if t.ResultTorrent == "" {
		status := strings.TrimSpace(t.Status)
		msg := strings.TrimSpace(t.StatusMessage)
		if strings.EqualFold(status, "FAILED") {
			if msg == "" {
				msg = "task failed"
			}
			return &pb.GetTaskResultResponse{Success: false, StatusMessage: msg}, nil
		}
		if status != "" {
			if msg != "" {
				return &pb.GetTaskResultResponse{Success: false, StatusMessage: fmt.Sprintf("%s: %s", status, msg)}, nil
			}
			return &pb.GetTaskResultResponse{Success: false, StatusMessage: fmt.Sprintf("%s: result not ready", status)}, nil
		}
		return &pb.GetTaskResultResponse{Success: false, StatusMessage: "result not ready"}, nil
	}
	return &pb.GetTaskResultResponse{Success: true, StatusMessage: "ok", ResultTorrent: t.ResultTorrent}, nil
}

func (m *masterNodeServer) GetTasklog(ctx context.Context, req *pb.TasklogRequest) (*pb.TasklogResponse, error) {
	_ = ctx
	owner, err := m.validateTokenFromMasterRequest(req.GetToken())
	if err != nil {
		return &pb.TasklogResponse{Success: false, Log: err.Error()}, nil
	}
	if req.GetTaskId() == "" {
		return &pb.TasklogResponse{Success: false, Log: "task_id is required"}, nil
	}
	t, ok := m.getTask(req.GetTaskId())
	if !ok || t.Owner != owner {
		return &pb.TasklogResponse{Success: false, Log: "task not found"}, nil
	}
	if t.Output == "" {
		return &pb.TasklogResponse{Success: false, Log: "log not ready"}, nil
	}
	return &pb.TasklogResponse{Success: true, Log: t.Output}, nil
}

func (w *workerIngressServer) TaskOutputUpload(ctx context.Context, req *pb.TaskOutputUploadRequest) (*pb.TaskOutputUploadResponse, error) {
	_ = ctx
	if req.GetTaskId() == "" {
		return &pb.TaskOutputUploadResponse{Success: false, StatusMessage: "task_id is required"}, nil
	}
	if _, err := w.master.validateWorkerIngressToken(req.GetTaskId(), req.GetToken()); err != nil {
		return &pb.TaskOutputUploadResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if req.GetOutput() == "" {
		return &pb.TaskOutputUploadResponse{Success: false, StatusMessage: "output is required"}, nil
	}
	if ok := w.master.setTaskOutput(req.GetTaskId(), req.GetOutput()); !ok {
		return &pb.TaskOutputUploadResponse{Success: false, StatusMessage: "task not found"}, nil
	}
	return &pb.TaskOutputUploadResponse{Success: true, StatusMessage: "uploaded"}, nil
}

func (w *workerIngressServer) TaskResultUpload(ctx context.Context, req *pb.TaskResultUploadRequest) (*pb.TaskResultUploadResponse, error) {
	_ = ctx
	if req.GetTaskId() == "" {
		return &pb.TaskResultUploadResponse{Success: false, StatusMessage: "task_id is required"}, nil
	}
	if _, err := w.master.validateWorkerIngressToken(req.GetTaskId(), req.GetToken()); err != nil {
		return &pb.TaskResultUploadResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if req.GetResultTorrent() == "" {
		return &pb.TaskResultUploadResponse{Success: false, StatusMessage: "result_torrent is required"}, nil
	}
	if ok, msg := w.master.setTaskResult(req.GetTaskId(), req.GetResultTorrent()); !ok {
		return &pb.TaskResultUploadResponse{Success: false, StatusMessage: msg}, nil
	}
	return &pb.TaskResultUploadResponse{Success: true, StatusMessage: "uploaded"}, nil
}

func (w *workerIngressServer) TaskUsage(ctx context.Context, req *pb.TaskUsageRequest) (*pb.TaskUsageResponse, error) {
	_ = ctx
	if req.GetTaskId() == "" {
		return &pb.TaskUsageResponse{Success: false, StatusMessage: "task_id is required"}, nil
	}
	if _, err := w.master.validateWorkerIngressToken(req.GetTaskId(), req.GetToken()); err != nil {
		return &pb.TaskUsageResponse{Success: false, StatusMessage: err.Error()}, nil
	}
	if !w.master.setTaskUsage(req.GetTaskId(), req.GetCpuUsage(), req.GetMemoryUsage(), req.GetGpuUsage(), req.GetGpuMemoryUsage()) {
		return &pb.TaskUsageResponse{Success: false, StatusMessage: "task not found"}, nil
	}
	return &pb.TaskUsageResponse{Success: true, StatusMessage: "updated"}, nil
}

// generateMagnetFromZip generates a simple magnet link from zip file data
// generateMagnetFromZipStream generates a magnet link from a zip file using streaming
// to avoid loading the entire file into memory
func generateMagnetFromZipStream(filename string, reader io.Reader, maxSize int64) (string, int64, error) {
	// Use SHA1 hash as info_hash (simplified, not actual torrent)
	hasher := sha1.New()

	// Use LimitReader to enforce size limit while reading
	limitedReader := io.LimitReader(reader, maxSize+1)

	// Stream the file through the hasher
	bytesRead, err := io.Copy(hasher, limitedReader)
	if err != nil {
		return "", 0, fmt.Errorf("failed to read file: %w", err)
	}

	// Check if file exceeded the limit
	if bytesRead > maxSize {
		return "", 0, fmt.Errorf("file size exceeds maximum allowed size of %d bytes", maxSize)
	}

	hashHex := fmt.Sprintf("%040x", hasher.Sum(nil))
	dn := strings.TrimSuffix(filename, ".zip")
	magnetLink := fmt.Sprintf("magnet:?xt=urn:btih:%s&dn=%s&xl=%d", hashHex, dn, bytesRead)

	return magnetLink, bytesRead, nil
}

func startHTTPAuthServer(auth *userAuthServer, master *masterNodeServer) {
	httpAddr := ":8081"
	if v := os.Getenv("NODEPOOL_HTTP_ADDR"); v != "" {
		httpAddr = v
	}
	bearerUser := func(r *http.Request) (string, error) {
		h := r.Header.Get("Authorization")
		if !strings.HasPrefix(h, "Bearer ") {
			return "", fmt.Errorf("missing bearer token")
		}
		return auth.usernameFromToken(strings.TrimPrefix(h, "Bearer "))
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/api/register", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		var body struct{ Username, Password string }
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		success, msg := auth.registerUser(body.Username, body.Password)
		_ = json.NewEncoder(w).Encode(map[string]any{"success": success, "status_message": msg})
		nodepoolLog(fmt.Sprintf("user register username=%s success=%v msg=%s", body.Username, success, msg))
	})
	mux.HandleFunc("/api/login", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		var body struct{ Username, Password string }
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		resp, _ := auth.Login(r.Context(), &pb.LoginRequest{Username: body.Username, Password: body.Password})
		_ = json.NewEncoder(w).Encode(map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage(), "token": resp.GetToken()})
		nodepoolLog(fmt.Sprintf("user login username=%s success=%v msg=%s", body.Username, resp.GetSuccess(), resp.GetStatusMessage()))
	})
	mux.HandleFunc("/api/balance", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "GET, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		user, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		tok := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		resp, _ := auth.GetBalance(r.Context(), &pb.GetBalanceRequest{Username: user, Token: tok})
		_ = json.NewEncoder(w).Encode(map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage(), "balance": resp.GetBalance()})
	})
	mux.HandleFunc("/api/tasks", func(w http.ResponseWriter, r *http.Request) {
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
		user, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		qv := r.URL.Query()
		status := strings.TrimSpace(qv.Get("status"))
		keyword := strings.TrimSpace(qv.Get("q"))
		fromTS, _ := strconv.ParseInt(strings.TrimSpace(qv.Get("from_ts")), 10, 64)
		toTS, _ := strconv.ParseInt(strings.TrimSpace(qv.Get("to_ts")), 10, 64)
		limit, _ := strconv.Atoi(strings.TrimSpace(qv.Get("limit")))
		offset, _ := strconv.Atoi(strings.TrimSpace(qv.Get("offset")))
		sortBy := strings.TrimSpace(qv.Get("sort_by"))
		order := strings.TrimSpace(strings.ToLower(qv.Get("order")))
		asc := order == "asc"

		tasks, total := master.listTasksByOwnerFiltered(user, status, keyword, fromTS, toTS, limit, offset, sortBy, asc)
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "tasks": tasks, "total": total, "limit": limit, "offset": offset})
	})
	mux.HandleFunc("/api/transfers", func(w http.ResponseWriter, r *http.Request) {
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
		user, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		if auth.db == nil {
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "db not initialized"})
			return
		}
		limit := envInt("NODEPOOL_TRANSFERS_LIMIT", 100)
		if q := strings.TrimSpace(r.URL.Query().Get("limit")); q != "" {
			if n, err := strconv.Atoi(q); err == nil && n > 0 && n <= 1000 {
				limit = n
			}
		}
		taskID := strings.TrimSpace(r.URL.Query().Get("task_id"))
		fromTS := strings.TrimSpace(r.URL.Query().Get("from_ts"))
		toTS := strings.TrimSpace(r.URL.Query().Get("to_ts"))
		aggregate := strings.EqualFold(strings.TrimSpace(r.URL.Query().Get("aggregate")), "1") || strings.EqualFold(strings.TrimSpace(r.URL.Query().Get("aggregate")), "true")
		rows, err := auth.db.Query("SELECT id,task_id,payer,payee,amount,created_at FROM cpt_transfers WHERE (payer=$1 OR payee=$2) AND (NULLIF($3,'') IS NULL OR task_id=$4) AND (NULLIF($5,'') IS NULL OR created_at >= to_timestamp(NULLIF($6,'')::bigint)) AND (NULLIF($7,'') IS NULL OR created_at <= to_timestamp(NULLIF($8,'')::bigint)) ORDER BY id DESC LIMIT $9", user, user, taskID, taskID, fromTS, fromTS, toTS, toTS, limit)
		if err != nil {
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		defer rows.Close()
		items := make([]map[string]any, 0)
		for rows.Next() {
			var id int64
			var tid, payer, payee, createdAt string
			var amount int64
			if err := rows.Scan(&id, &tid, &payer, &payee, &amount, &createdAt); err != nil {
				continue
			}
			items = append(items, map[string]any{
				"id":         id,
				"task_id":    tid,
				"payer":      payer,
				"payee":      payee,
				"amount":     amount,
				"created_at": createdAt,
			})
		}
		if aggregate {
			var totalIn int64
			var totalOut int64
			for _, it := range items {
				amount, _ := it["amount"].(int64)
				payer, _ := it["payer"].(string)
				payee, _ := it["payee"].(string)
				if payee == user {
					totalIn += amount
				}
				if payer == user {
					totalOut += amount
				}
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "transfers": items, "summary": map[string]any{"total_in": totalIn, "total_out": totalOut, "net": totalIn - totalOut, "count": len(items)}})
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "transfers": items})
	})
	mux.HandleFunc("/api/workers", func(w http.ResponseWriter, r *http.Request) {
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
		_, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		includeOffline := true
		if v := strings.TrimSpace(strings.ToLower(r.URL.Query().Get("include_offline"))); v != "" {
			includeOffline = v == "1" || v == "true" || v == "yes"
		}
		workers, err := master.svc.ListWorkers(r.Context(), includeOffline)
		if err != nil {
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "workers": workers})
	})
	mux.HandleFunc("/api/create-torrent", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}

		_, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}

		// Parse multipart form with max 100MB
		if err := r.ParseMultipartForm(100 * 1024 * 1024); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "failed to parse form: " + err.Error()})
			return
		}

		file, header, err := r.FormFile("file")
		if err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "file required"})
			return
		}
		defer file.Close()

		if !strings.HasSuffix(strings.ToLower(header.Filename), ".zip") {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "only .zip files are supported"})
			return
		}

		// Use streaming to avoid loading entire file into RAM
		maxSize := int64(100 * 1024 * 1024) // 100MB
		magnetLink, bytesRead, err := generateMagnetFromZipStream(header.Filename, file, maxSize)
		if err != nil {
			if strings.Contains(err.Error(), "exceeds maximum") {
				w.WriteHeader(http.StatusRequestEntityTooLarge)
				_ = json.NewEncoder(w).Encode(map[string]any{
					"success":        false,
					"status_message": "file too large (max 100MB)",
				})
				return
			}
			w.WriteHeader(http.StatusInternalServerError)
			_ = json.NewEncoder(w).Encode(map[string]any{
				"success":        false,
				"status_message": "failed to process file",
			})
			return
		}

		_ = json.NewEncoder(w).Encode(map[string]any{
			"success":        true,
			"status_message": "torrent created",
			"torrent":        magnetLink,
			"magnet":         magnetLink,
			"torrent_name":   strings.TrimSuffix(header.Filename, ".zip"),
			"file_size":      bytesRead,
		})
	})
	mux.HandleFunc("/api/upload-task", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}

		_, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		tok := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")

		var body struct {
			TaskID      string `json:"task_id"`
			Torrent     string `json:"torrent"`
			MemoryGB    int32  `json:"memory_gb"`
			GPUMemoryGB int32  `json:"gpu_memory_gb"`
			HostCount   int32  `json:"host_count"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		if strings.TrimSpace(body.TaskID) == "" {
			body.TaskID = fmt.Sprintf("task-%d", time.Now().UnixNano())
		}

		resp, _ := master.UploadTask(r.Context(), &pb.UploadTaskRequest{
			TaskId:      body.TaskID,
			Torrent:     body.Torrent,
			MemoryGb:    body.MemoryGB,
			GpuMemoryGb: body.GPUMemoryGB,
			HostCount:   body.HostCount,
			Token:       tok,
		})

		_ = json.NewEncoder(w).Encode(map[string]any{
			"success":        resp.GetSuccess(),
			"status_message": resp.GetStatusMessage(),
			"task_id":        body.TaskID,
		})
	})
	mux.HandleFunc("/api/task/", func(w http.ResponseWriter, r *http.Request) {
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

		user, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}

		// Parse path: /api/task/{taskId}/log
		parts := strings.Split(strings.TrimPrefix(r.URL.Path, "/api/task/"), "/")
		if len(parts) < 2 || parts[0] == "" {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid path"})
			return
		}

		taskId := parts[0]
		action := parts[1]

		t, ok := master.getTask(taskId)
		if !ok || t.Owner != user {
			w.WriteHeader(http.StatusNotFound)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "task not found"})
			return
		}

		if action == "log" {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"success": true,
				"log":     t.Output,
			})
		} else {
			w.WriteHeader(http.StatusNotFound)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "endpoint not found"})
		}
	})
	mux.HandleFunc("/api/stop-task", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}

		_, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}

		tok := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")

		var body struct {
			TaskID string `json:"task_id"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid json"})
			return
		}

		// Call gRPC StopTask
		resp, _ := master.StopTask(r.Context(), &pb.StopTaskRequest{
			TaskId: body.TaskID,
			Token:  tok,
		})

		_ = json.NewEncoder(w).Encode(map[string]any{
			"success":        resp.GetSuccess(),
			"status_message": resp.GetStatusMessage(),
		})
	})
	mux.HandleFunc("/api/stop-tasks", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		_, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		tok := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		var body struct {
			TaskIDs []string `json:"task_ids"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		if len(body.TaskIDs) == 0 {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "task_ids required"})
			return
		}
		results := make([]map[string]any, 0, len(body.TaskIDs))
		successCount := 0
		for _, id := range body.TaskIDs {
			id = strings.TrimSpace(id)
			if id == "" {
				continue
			}
			resp, _ := master.StopTask(r.Context(), &pb.StopTaskRequest{TaskId: id, Token: tok})
			if resp.GetSuccess() {
				successCount++
			}
			results = append(results, map[string]any{"task_id": id, "success": resp.GetSuccess(), "status_message": resp.GetStatusMessage()})
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": successCount > 0, "stopped": successCount, "total": len(results), "results": results})
	})
	mux.HandleFunc("/api/remove-worker", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			w.WriteHeader(http.StatusMethodNotAllowed)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		_, err := bearerUser(r)
		if err != nil {
			w.WriteHeader(http.StatusUnauthorized)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		var body struct {
			WorkerID string `json:"worker_id"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		if strings.TrimSpace(body.WorkerID) == "" {
			w.WriteHeader(http.StatusBadRequest)
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": "worker_id required"})
			return
		}
		if err := master.svc.RemoveWorker(r.Context(), body.WorkerID); err != nil {
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		nodepoolLog(fmt.Sprintf("worker_removed worker_id=%s", body.WorkerID))
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "status_message": "worker removed", "worker_id": body.WorkerID})
	})
	go func() {
		log.Printf("HTTP auth server listening on %s", httpAddr)
		if err := http.ListenAndServe(httpAddr, requestLogMiddleware("nodepool-http", mux)); err != nil {
			log.Printf("HTTP auth server stopped: %v", err)
		}
	}()
}

func main() {
	dsn := strings.TrimSpace(os.Getenv("NODEPOOL_POSTGRES_DSN"))
	if dsn == "" {
		dsn = "postgres://hivemind:hivemind@localhost:5432/hivemind?sslmode=disable"
	}
	db, err := retryInitDB(dsn)
	if err != nil {
		log.Fatalf("init db failed: %v", err)
	}
	defer db.Close()

	jwtSecret := os.Getenv("NODEPOOL_JWT_SECRET")
	if jwtSecret == "" {
		jwtSecret = "dev-secret-change-me"
	}
	strictBTIH := strings.EqualFold(os.Getenv("NODEPOOL_STRICT_BTIH"), "1") || strings.EqualFold(os.Getenv("NODEPOOL_STRICT_BTIH"), "true")

	// Initialize Redis client for task metadata storage
	redisAddr := os.Getenv("NODEPOOL_REDIS_ADDR")
	if redisAddr == "" {
		redisAddr = "localhost:6379"
	}
	redisClient := redis.NewClient(&redis.Options{Addr: redisAddr})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	err = redisClient.Ping(ctx).Err()
	cancel()
	if err != nil {
		log.Printf("warning: redis connection failed (%v), continuing without redis", err)
		redisClient = nil
	} else {
		log.Printf("redis connected at %s", redisAddr)
	}

	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	authSrv := newUserAuthServer(db, jwtSecret)
	masterSrv := &masterNodeServer{svc: svc, auth: authSrv, db: db, redis: redisClient, strictBTIH: strictBTIH, taskToWorker: make(map[string]string), taskRoutes: make(map[string]map[string]string), tasks: make(map[string]*taskState), batchLeases: make(map[string]*batchLease)}
	masterSrv.loadTasksFromDB()
	taskTimeout := time.Duration(envInt("NODEPOOL_TASK_TIMEOUT_SEC", 30)) * time.Second
	maxRedispatch := envInt("NODEPOOL_MAX_REDISPATCH", 2)
	settlementInterval := time.Duration(envInt("NODEPOOL_SETTLEMENT_INTERVAL_SEC", 60)) * time.Second
	monitorCtx, monitorCancel := context.WithCancel(context.Background())
	defer monitorCancel()
	go masterSrv.startTaskTimeoutMonitor(monitorCtx, taskTimeout, maxRedispatch)
	go masterSrv.startPeriodicSettlementMonitor(monitorCtx, settlementInterval)
	workerIngress := &workerIngressServer{master: masterSrv}

	grpcServer := grpc.NewServer()
	pb.RegisterNodeManagerServiceServer(grpcServer, &nodeManagerServer{svc: svc})
	pb.RegisterMasterNodeServiceServer(grpcServer, masterSrv)
	pb.RegisterBatchRuntimeServiceServer(grpcServer, masterSrv)
	pb.RegisterWorkerNodeServiceServer(grpcServer, workerIngress)
	pb.RegisterUserServiceServer(grpcServer, authSrv)
	enableHTTPAuth := strings.EqualFold(os.Getenv("NODEPOOL_ENABLE_HTTP_AUTH"), "1") || strings.EqualFold(os.Getenv("NODEPOOL_ENABLE_HTTP_AUTH"), "true")
	if enableHTTPAuth {
		startHTTPAuthServer(authSrv, masterSrv)
	} else {
		log.Printf("nodepool HTTP auth server disabled (gRPC-only mode)")
	}

	addr := ":50051"
	if v := os.Getenv("NODEPOOL_ADDR"); v != "" {
		addr = v
	}
	lis, err := net.Listen("tcp", addr)
	if err != nil {
		log.Fatalf("failed to listen: %v", err)
	}
	log.Printf("gRPC nodepool server listening on %s", addr)
	if err := grpcServer.Serve(lis); err != nil {
		log.Fatalf("gRPC server failed: %v", err)
	}
}

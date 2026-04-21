package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"hivemind/services/master/internal/bt"
	pb "hivemind/services/nodepool/pb"
)

type grpcClients struct {
	conn   *grpc.ClientConn
	user   pb.UserServiceClient
	master pb.MasterNodeServiceClient
	node   pb.NodeManagerServiceClient
}

func mustDial(nodepoolGRPC string) *grpcClients {
	conn, err := grpc.Dial(nodepoolGRPC, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("dial nodepool grpc failed: %v", err)
	}
	return &grpcClients{
		conn:   conn,
		user:   pb.NewUserServiceClient(conn),
		master: pb.NewMasterNodeServiceClient(conn),
		node:   pb.NewNodeManagerServiceClient(conn),
	}
}

func cors(w http.ResponseWriter) {
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
	w.Header().Set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
}

func withTimeout(parent context.Context) (context.Context, context.CancelFunc) {
	return context.WithTimeout(parent, 5*time.Second)
}

func bearerToken(r *http.Request) (string, error) {
	h := r.Header.Get("Authorization")
	if !strings.HasPrefix(h, "Bearer ") {
		return "", fmt.Errorf("missing bearer token")
	}
	return strings.TrimPrefix(h, "Bearer "), nil
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
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
func main() {
	addr := os.Getenv("MASTER_HTTP_ADDR")
	if addr == "" {
		addr = ":8082"
	}

	nodepoolGRPC := os.Getenv("NODEPOOL_GRPC_ADDR")
	if nodepoolGRPC == "" {
		nodepoolGRPC = "localhost:50051"
	}
	nodepoolHTTPBase := strings.TrimRight(os.Getenv("NODEPOOL_HTTP_BASE"), "/")
	if nodepoolHTTPBase == "" {
		nodepoolHTTPBase = "http://localhost:8081"
	}
	clients := mustDial(nodepoolGRPC)
	defer clients.conn.Close()
	callNodepoolHTTP := func(ctx context.Context, method, path, bearer string, payload any) (map[string]any, int, error) {
		var body io.Reader
		if payload != nil {
			b, err := json.Marshal(payload)
			if err != nil {
				return nil, 0, err
			}
			body = bytes.NewReader(b)
		}
		req, err := http.NewRequestWithContext(ctx, method, nodepoolHTTPBase+path, body)
		if err != nil {
			return nil, 0, err
		}
		if payload != nil {
			req.Header.Set("Content-Type", "application/json")
		}
		if strings.TrimSpace(bearer) != "" {
			req.Header.Set("Authorization", "Bearer "+bearer)
		}
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			return nil, 0, err
		}
		defer resp.Body.Close()
		out := map[string]any{}
		_ = json.NewDecoder(resp.Body).Decode(&out)
		return out, resp.StatusCode, nil
	}
	validateTokenUser := func(ctx context.Context, username, tok string) error {
		resp, err := clients.user.GetBalance(ctx, &pb.GetBalanceRequest{Username: username, Token: tok})
		if err != nil {
			return err
		}
		if !resp.GetSuccess() {
			return fmt.Errorf("%s", resp.GetStatusMessage())
		}
		return nil
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/api/register", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		var body struct {
			Username string `json:"username"`
			Password string `json:"password"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, status, err := callNodepoolHTTP(ctx, http.MethodPost, "/api/register", "", body)
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, status, resp)
	})
	mux.HandleFunc("/api/login", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		var body struct {
			Username string `json:"username"`
			Password string `json:"password"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, err := clients.user.Login(ctx, &pb.LoginRequest{Username: body.Username, Password: body.Password})
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage(), "token": resp.GetToken()})
	})

	mux.HandleFunc("/api/balance", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, err := clients.user.GetBalance(ctx, &pb.GetBalanceRequest{Token: tok})
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage(), "balance": resp.GetBalance()})
	})

	mux.HandleFunc("/api/tasks", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodGet {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		path := "/api/tasks"
		if r.URL.RawQuery != "" {
			path += "?" + r.URL.RawQuery
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, status, err := callNodepoolHTTP(ctx, http.MethodGet, path, tok, nil)
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, status, resp)
	})

	mux.HandleFunc("/api/transfers", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodGet {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		path := "/api/transfers"
		if r.URL.RawQuery != "" {
			path += "?" + r.URL.RawQuery
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, status, err := callNodepoolHTTP(ctx, http.MethodGet, path, tok, nil)
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, status, resp)
	})

	mux.HandleFunc("/api/workers", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodGet {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		path := "/api/workers"
		if r.URL.RawQuery != "" {
			path += "?" + r.URL.RawQuery
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, status, err := callNodepoolHTTP(ctx, http.MethodGet, path, tok, nil)
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, status, resp)
	})

	mux.HandleFunc("/api/upload-task", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}

		// Support two modes: multipart file upload (form) or JSON body with torrent URL/magnet
		ct := r.Header.Get("Content-Type")
		var taskID string
		var torrentStr string
		var memoryGB int32
		var gpuMemoryGB int32
		var hostCount int32

		if strings.HasPrefix(strings.ToLower(ct), "multipart/form-data") {
			if err := r.ParseMultipartForm(200 * 1024 * 1024); err != nil {
				writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "failed to parse form: " + err.Error()})
				return
			}
			// optional fields
			taskID = strings.TrimSpace(r.FormValue("task_id"))
			if taskID == "" {
				taskID = fmt.Sprintf("task-%d", time.Now().UnixNano())
			}
			if v := strings.TrimSpace(r.FormValue("memory_gb")); v != "" {
				if iv, err := strconv.Atoi(v); err == nil {
					memoryGB = int32(iv)
				}
			}
			if v := strings.TrimSpace(r.FormValue("gpu_memory_gb")); v != "" {
				if iv, err := strconv.Atoi(v); err == nil {
					gpuMemoryGB = int32(iv)
				}
			}
			if v := strings.TrimSpace(r.FormValue("host_count")); v != "" {
				if iv, err := strconv.Atoi(v); err == nil {
					hostCount = int32(iv)
				}
			}
			file, header, err := r.FormFile("file")
			if err != nil {
				writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "file required"})
				return
			}
			defer file.Close()
			// only accept .zip payloads for now
			if !strings.HasSuffix(strings.ToLower(header.Filename), ".zip") {
				writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "only .zip files are supported"})
				return
			}
			fileData, err := io.ReadAll(file)
			if err != nil {
				writeJSON(w, http.StatusInternalServerError, map[string]any{"success": false, "status_message": "failed to read file"})
				return
			}
			announce := os.Getenv("BT_ANNOUNCE")
			announceListRaw := os.Getenv("BT_ANNOUNCE_LIST")
			announceList := make([]string, 0)
			for _, it := range strings.Split(announceListRaw, ",") {
				it = strings.TrimSpace(it)
				if it != "" {
					announceList = append(announceList, it)
				}
			}
			meta, torrentBytes, err := bt.CreateTorrentFromPayloadWithOptions(header.Filename, fileData, announce, announceList)
			if err != nil {
				writeJSON(w, http.StatusInternalServerError, map[string]any{"success": false, "status_message": err.Error()})
				return
			}
			torrentDir := os.Getenv("BT_TORRENT_DIR")
			if strings.TrimSpace(torrentDir) == "" {
				torrentDir = "bt_torrents"
			}
			if err := bt.PersistTorrent(torrentDir, &meta, torrentBytes); err != nil {
				writeJSON(w, http.StatusInternalServerError, map[string]any{"success": false, "status_message": "persist torrent failed: " + err.Error()})
				return
			}
			_ = bt.PersistMeta(os.Getenv("BT_META_DIR"), meta)
			torrentStr = meta.Magnet
		} else {
			var body struct {
				TaskID      string `json:"task_id"`
				Torrent     string `json:"torrent"`
				MemoryGB    int32  `json:"memory_gb"`
				GPUMemoryGB int32  `json:"gpu_memory_gb"`
				HostCount   int32  `json:"host_count"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
				return
			}
			taskID = strings.TrimSpace(body.TaskID)
			if taskID == "" {
				taskID = fmt.Sprintf("task-%d", time.Now().UnixNano())
			}
			torrentStr = strings.TrimSpace(body.Torrent)
			memoryGB = body.MemoryGB
			gpuMemoryGB = body.GPUMemoryGB
			hostCount = body.HostCount
		}

		// allow relative torrent paths (e.g. /api/torrents/xxx.torrent)
		if strings.HasPrefix(torrentStr, "/") {
			scheme := "http"
			if r.TLS != nil || strings.EqualFold(r.Header.Get("X-Forwarded-Proto"), "https") {
				scheme = "https"
			}
			host := r.Host
			if host == "" {
				host = "localhost"
			}
			torrentStr = scheme + "://" + host + torrentStr
		}

		// validate torrentStr: magnet or http(s) url
		if _, err := bt.ParseMagnet(torrentStr); err != nil {
			if !(strings.HasPrefix(strings.ToLower(torrentStr), "http://") || strings.HasPrefix(strings.ToLower(torrentStr), "https://")) {
				writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid torrent source: must be magnet or http(s) .torrent url"})
				return
			}
		}

		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, err := clients.master.UploadTask(ctx, &pb.UploadTaskRequest{TaskId: taskID, Torrent: torrentStr, MemoryGb: memoryGB, GpuMemoryGb: gpuMemoryGB, HostCount: hostCount, Token: tok})
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error(), "task_id": taskID})
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage(), "task_id": taskID})
	})

	mux.HandleFunc("/api/register-worker", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		var body struct {
			Username    string `json:"username"`
			Ip          string `json:"ip"`
			CpuCores    int32  `json:"cpu_cores"`
			MemoryGb    int32  `json:"memory_gb"`
			CpuScore    int32  `json:"cpu_score"`
			GpuScore    int32  `json:"gpu_score"`
			GpuMemoryGb int32  `json:"gpu_memory_gb"`
			Location    string `json:"location"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		if strings.TrimSpace(body.Username) == "" || strings.TrimSpace(body.Ip) == "" {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "username/ip required"})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		if err := validateTokenUser(ctx, body.Username, tok); err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": "token-user mismatch or invalid token"})
			return
		}
		resp, err := clients.node.RegisterWorkerNode(ctx, &pb.RegisterWorkerNodeRequest{
			Username:    body.Username,
			Ip:          body.Ip,
			CpuCores:    body.CpuCores,
			MemoryGb:    body.MemoryGb,
			CpuScore:    body.CpuScore,
			GpuScore:    body.GpuScore,
			GpuMemoryGb: body.GpuMemoryGb,
			Location:    body.Location,
		})
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage()})
	})

	mux.HandleFunc("/api/stop-task", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		var body struct {
			TaskID string `json:"task_id"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, err := clients.master.StopTask(ctx, &pb.StopTaskRequest{TaskId: body.TaskID, Token: tok})
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage()})
	})

	mux.HandleFunc("/api/stop-tasks", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		var body struct {
			TaskIDs []string `json:"task_ids"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, status, err := callNodepoolHTTP(ctx, http.MethodPost, "/api/stop-tasks", tok, body)
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, status, resp)
	})

	mux.HandleFunc("/api/remove-worker", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		var body struct {
			WorkerID string `json:"worker_id"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid json"})
			return
		}
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		resp, status, err := callNodepoolHTTP(ctx, http.MethodPost, "/api/remove-worker", tok, body)
		if err != nil {
			writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		writeJSON(w, status, resp)
	})

	mux.HandleFunc("/api/task/", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		tok, err := bearerToken(r)
		if err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		parts := strings.Split(strings.TrimPrefix(r.URL.Path, "/api/task/"), "/")
		if len(parts) < 2 || parts[0] == "" {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid path"})
			return
		}
		taskID := parts[0]
		action := parts[1]
		ctx, cancel := withTimeout(r.Context())
		defer cancel()
		switch action {
		case "log":
			resp, err := clients.master.GetTasklog(ctx, &pb.TasklogRequest{TaskId: taskID, Token: tok})
			if err != nil {
				writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
				return
			}
			if resp.GetSuccess() {
				writeJSON(w, http.StatusOK, map[string]any{"success": true, "log": resp.GetLog()})
				return
			}
			// when GetTasklog failed, return the log field as status_message for clarity
			writeJSON(w, http.StatusOK, map[string]any{"success": false, "status_message": resp.GetLog()})
		case "result":
			resp, err := clients.master.GetTaskResult(ctx, &pb.GetTaskResultRequest{TaskId: taskID, Token: tok})
			if err != nil {
				writeJSON(w, http.StatusBadGateway, map[string]any{"success": false, "status_message": err.Error()})
				return
			}
			writeJSON(w, http.StatusOK, map[string]any{"success": resp.GetSuccess(), "status_message": resp.GetStatusMessage(), "result_torrent": resp.GetResultTorrent()})
		default:
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid task action"})
		}
	})

	mux.HandleFunc("/api/create-torrent", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodPost {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		if _, err := bearerToken(r); err != nil {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		if err := r.ParseMultipartForm(100 * 1024 * 1024); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "failed to parse form: " + err.Error()})
			return
		}
		file, header, err := r.FormFile("file")
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "file required"})
			return
		}
		defer file.Close()
		if !strings.HasSuffix(strings.ToLower(header.Filename), ".zip") {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "only .zip files are supported"})
			return
		}
		fileData, err := io.ReadAll(file)
		if err != nil {
			writeJSON(w, http.StatusInternalServerError, map[string]any{"success": false, "status_message": "failed to read file"})
			return
		}
		announce := os.Getenv("BT_ANNOUNCE")
		announceListRaw := os.Getenv("BT_ANNOUNCE_LIST")
		announceList := make([]string, 0)
		for _, it := range strings.Split(announceListRaw, ",") {
			it = strings.TrimSpace(it)
			if it != "" {
				announceList = append(announceList, it)
			}
		}
		meta, torrentBytes, err := bt.CreateTorrentFromPayloadWithOptions(header.Filename, fileData, announce, announceList)
		if err != nil {
			writeJSON(w, http.StatusInternalServerError, map[string]any{"success": false, "status_message": err.Error()})
			return
		}
		torrentDir := os.Getenv("BT_TORRENT_DIR")
		if strings.TrimSpace(torrentDir) == "" {
			torrentDir = "bt_torrents"
		}
		if err := bt.PersistTorrent(torrentDir, &meta, torrentBytes); err != nil {
			writeJSON(w, http.StatusInternalServerError, map[string]any{"success": false, "status_message": "persist torrent failed: " + err.Error()})
			return
		}
		_ = bt.PersistMeta(os.Getenv("BT_META_DIR"), meta)
		torrentPath := fmt.Sprintf("/api/torrents/%s.torrent?ih=%s", meta.InfoHash, meta.InfoHash)
		publicBase := strings.TrimRight(os.Getenv("BT_PUBLIC_BASE_URL"), "/")
		if publicBase == "" {
			scheme := "http"
			if r.TLS != nil || strings.EqualFold(r.Header.Get("X-Forwarded-Proto"), "https") {
				scheme = "https"
			}
			host := r.Host
			if host == "" {
				host = "localhost"
			}
			publicBase = scheme + "://" + host
		}
		torrentURL := publicBase + torrentPath
		writeJSON(w, http.StatusOK, map[string]any{
			"success":        true,
			"status_message": "torrent created",
			"torrent":        meta.Magnet,
			"magnet":         meta.Magnet,
			"torrent_name":   meta.DisplayName,
			"info_hash":      meta.InfoHash,
			"length":         meta.Length,
			"piece_length":   meta.PieceLength,
			"file_count":     meta.FileCount,
			"trackers":       meta.Trackers,
			"torrent_file":   torrentPath,
			"torrent_url":    torrentURL,
		})
	})

	mux.HandleFunc("/api/torrents/", func(w http.ResponseWriter, r *http.Request) {
		cors(w)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if r.Method != http.MethodGet {
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"success": false, "status_message": "method not allowed"})
			return
		}
		requireAuth := strings.EqualFold(os.Getenv("BT_TORRENT_REQUIRE_AUTH"), "1") || strings.EqualFold(os.Getenv("BT_TORRENT_REQUIRE_AUTH"), "true")
		if requireAuth {
			if _, err := bearerToken(r); err != nil {
				writeJSON(w, http.StatusUnauthorized, map[string]any{"success": false, "status_message": err.Error()})
				return
			}
		}
		name := strings.TrimPrefix(r.URL.Path, "/api/torrents/")
		if !strings.HasSuffix(name, ".torrent") || strings.Contains(name, "/") || strings.Contains(name, "..") {
			writeJSON(w, http.StatusBadRequest, map[string]any{"success": false, "status_message": "invalid torrent path"})
			return
		}
		torrentDir := os.Getenv("BT_TORRENT_DIR")
		if strings.TrimSpace(torrentDir) == "" {
			torrentDir = "bt_torrents"
		}
		fp := filepath.Join(torrentDir, name)
		b, err := os.ReadFile(fp)
		if err != nil {
			writeJSON(w, http.StatusNotFound, map[string]any{"success": false, "status_message": "torrent file not found"})
			return
		}
		w.Header().Set("Content-Type", "application/x-bittorrent")
		w.Header().Set("Content-Disposition", "attachment; filename=\""+name+"\"")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(b)
	})

	mux.HandleFunc("/healthz", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"service":"master","upstream_grpc":"` + nodepoolGRPC + `"}`))
	})

	log.Printf("master http server listening on %s, nodepool grpc=%s", addr, nodepoolGRPC)
	if err := http.ListenAndServe(addr, requestLogMiddleware("master-http", mux)); err != nil {
		log.Fatalf("master http server failed: %v", err)
	}
}

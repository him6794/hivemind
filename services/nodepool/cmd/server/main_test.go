package main

import (
	"context"
	"fmt"
	"net"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"

	"hivemind/services/nodepool/internal/repository"
	"hivemind/services/nodepool/internal/service"
	"hivemind/services/nodepool/pb"
)

const bufSize = 1024 * 1024

func testPostgresDSN(t *testing.T) string {
	t.Helper()
	dsn := strings.TrimSpace(os.Getenv("NODEPOOL_TEST_POSTGRES_DSN"))
	if dsn == "" {
		dsn = strings.TrimSpace(os.Getenv("NODEPOOL_POSTGRES_DSN"))
	}
	if dsn == "" {
		t.Skip("NODEPOOL_TEST_POSTGRES_DSN (or NODEPOOL_POSTGRES_DSN) is required for PostgreSQL tests")
	}
	return dsn
}

func TestNodeManagerGRPC_RegisterAndReportStatus(t *testing.T) {
	t.Parallel()

	lis := bufconn.Listen(bufSize)
	grpcServer := grpc.NewServer()

	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	srv := &nodeManagerServer{svc: svc}
	pb.RegisterNodeManagerServiceServer(grpcServer, srv)

	go func() {
		_ = grpcServer.Serve(lis)
	}()
	t.Cleanup(func() {
		grpcServer.Stop()
		_ = lis.Close()
	})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	conn, err := grpc.DialContext(
		ctx,
		"bufnet",
		grpc.WithContextDialer(func(context.Context, string) (net.Conn, error) {
			return lis.Dial()
		}),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		t.Fatalf("dial bufnet: %v", err)
	}
	defer conn.Close()

	client := pb.NewNodeManagerServiceClient(conn)

	regResp, err := client.RegisterWorkerNode(ctx, &pb.RegisterWorkerNodeRequest{
		Username:    "worker-1",
		Ip:          "127.0.0.1",
		CpuCores:    8,
		MemoryGb:    32,
		CpuScore:    100,
		GpuScore:    200,
		GpuMemoryGb: 12,
		Location:    "local",
	})
	if err != nil {
		t.Fatalf("RegisterWorkerNode err: %v", err)
	}
	if !regResp.GetSuccess() {
		t.Fatalf("RegisterWorkerNode failed: %s", regResp.GetStatusMessage())
	}

	hbResp, err := client.ReportStatus(ctx, &pb.RunningStatusRequest{
		Username: "worker-1",
		Status:   "Idle",
	})
	if err != nil {
		t.Fatalf("ReportStatus err: %v", err)
	}
	if !hbResp.GetSuccess() {
		t.Fatalf("ReportStatus failed: %s", hbResp.GetStatusMessage())
	}
}

type mockWorkerServer struct {
	pb.UnimplementedWorkerNodeServiceServer
	lastExecuteReq *pb.ExecuteTaskRequest
	lastStopReq    *pb.StopTaskExecutionRequest
}

func (m *mockWorkerServer) ExecuteTask(ctx context.Context, req *pb.ExecuteTaskRequest) (*pb.ExecuteTaskResponse, error) {
	_ = ctx
	m.lastExecuteReq = req
	return &pb.ExecuteTaskResponse{Success: true, StatusMessage: "accepted"}, nil
}

func (m *mockWorkerServer) StopTaskExecution(ctx context.Context, req *pb.StopTaskExecutionRequest) (*pb.StopTaskExecutionResponse, error) {
	_ = ctx
	m.lastStopReq = req
	return &pb.StopTaskExecutionResponse{Success: true, StatusMessage: "stopped"}, nil
}

func (m *mockWorkerServer) TaskOutput(ctx context.Context, req *pb.TaskOutputRequest) (*pb.TaskOutputResponse, error) {
	_ = ctx
	_ = req
	return &pb.TaskOutputResponse{Success: false, StatusMessage: "probe ok"}, nil
}

type stressWorkerServer struct {
	pb.UnimplementedWorkerNodeServiceServer
	executeCalls int64
	probeCalls   int64
	stopCalls    int64
}

func (s *stressWorkerServer) ExecuteTask(ctx context.Context, req *pb.ExecuteTaskRequest) (*pb.ExecuteTaskResponse, error) {
	_ = ctx
	if strings.TrimSpace(req.GetTaskId()) == "" {
		return &pb.ExecuteTaskResponse{Success: false, StatusMessage: "task_id required"}, nil
	}
	atomic.AddInt64(&s.executeCalls, 1)
	return &pb.ExecuteTaskResponse{Success: true, StatusMessage: "accepted"}, nil
}

func (s *stressWorkerServer) TaskOutput(ctx context.Context, req *pb.TaskOutputRequest) (*pb.TaskOutputResponse, error) {
	_ = ctx
	_ = req
	atomic.AddInt64(&s.probeCalls, 1)
	return &pb.TaskOutputResponse{Success: false, StatusMessage: "probe ok"}, nil
}

func (s *stressWorkerServer) StopTaskExecution(ctx context.Context, req *pb.StopTaskExecutionRequest) (*pb.StopTaskExecutionResponse, error) {
	_ = ctx
	_ = req
	atomic.AddInt64(&s.stopCalls, 1)
	return &pb.StopTaskExecutionResponse{Success: true, StatusMessage: "stopped"}, nil
}

func TestNormalUserConcurrentLifecycleStress(t *testing.T) {
	workerLis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("worker listen: %v", err)
	}
	workerGRPC := grpc.NewServer()
	stressWorker := &stressWorkerServer{}
	pb.RegisterWorkerNodeServiceServer(workerGRPC, stressWorker)
	go func() { _ = workerGRPC.Serve(workerLis) }()
	t.Cleanup(func() {
		workerGRPC.Stop()
		_ = workerLis.Close()
	})

	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	authSrv := newUserAuthServer(nil, "stress-secret")
	masterSrv := &masterNodeServer{svc: svc, auth: authSrv, taskToWorker: make(map[string]string), taskRoutes: make(map[string]map[string]string), tasks: make(map[string]*taskState)}
	ingress := &workerIngressServer{master: masterSrv}
	nodeSrv := &nodeManagerServer{svc: svc}

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	token, err := authSrv.issueToken("worker1")
	if err != nil {
		t.Fatalf("issue token: %v", err)
	}
	regResp, err := nodeSrv.RegisterWorkerNode(ctx, &pb.RegisterWorkerNodeRequest{
		Username:    "worker1",
		Ip:          workerLis.Addr().String(),
		CpuCores:    8,
		MemoryGb:    32,
		CpuScore:    200,
		GpuScore:    100,
		GpuMemoryGb: 16,
		Location:    "local",
	})
	if err != nil {
		t.Fatalf("register worker err: %v", err)
	}
	if !regResp.GetSuccess() {
		t.Fatalf("register worker failed: %s", regResp.GetStatusMessage())
	}

	const taskCount = 80
	const expectedBTIH = "0123456789abcdef0123456789abcdef01234567"
	start := time.Now()
	var wg sync.WaitGroup
	errs := make(chan string, taskCount)
	for i := 0; i < taskCount; i++ {
		i := i
		wg.Add(1)
		go func() {
			defer wg.Done()
			taskID := fmt.Sprintf("normal-user-stress-%03d", i)
			uploadResp, err := masterSrv.UploadTask(ctx, &pb.UploadTaskRequest{
				TaskId:      taskID,
				Torrent:     "magnet:?xt=urn:btih:" + expectedBTIH,
				MemoryGb:    2,
				CpuScore:    100,
				GpuScore:    0,
				GpuMemoryGb: 0,
				HostCount:   1,
				Token:       token,
			})
			if err != nil || !uploadResp.GetSuccess() {
				errs <- fmt.Sprintf("%s upload failed: resp=%v err=%v", taskID, uploadResp, err)
				return
			}
			usageResp, err := ingress.TaskUsage(ctx, &pb.TaskUsageRequest{TaskId: taskID, CpuUsage: 12.5, MemoryUsage: 256, GpuUsage: 0, GpuMemoryUsage: 0, Token: token})
			if err != nil || !usageResp.GetSuccess() {
				errs <- fmt.Sprintf("%s usage failed: resp=%v err=%v", taskID, usageResp, err)
				return
			}
			outResp, err := ingress.TaskOutputUpload(ctx, &pb.TaskOutputUploadRequest{TaskId: taskID, Output: "normal progress update", Token: token})
			if err != nil || !outResp.GetSuccess() {
				errs <- fmt.Sprintf("%s output failed: resp=%v err=%v", taskID, outResp, err)
				return
			}
			resultResp, err := ingress.TaskResultUpload(ctx, &pb.TaskResultUploadRequest{TaskId: taskID, ResultTorrent: "magnet:?xt=urn:btih:" + expectedBTIH, Token: token})
			if err != nil || !resultResp.GetSuccess() {
				errs <- fmt.Sprintf("%s result upload failed: resp=%v err=%v", taskID, resultResp, err)
				return
			}
			getResp, err := masterSrv.GetTaskResult(ctx, &pb.GetTaskResultRequest{TaskId: taskID, Token: token})
			if err != nil || !getResp.GetSuccess() || !strings.Contains(getResp.GetResultTorrent(), expectedBTIH) {
				errs <- fmt.Sprintf("%s get result failed: resp=%v err=%v", taskID, getResp, err)
				return
			}
		}()
	}
	wg.Wait()
	close(errs)
	for msg := range errs {
		t.Error(msg)
	}
	if t.Failed() {
		return
	}

	listResp, err := masterSrv.GetAllUserTasks(ctx, &pb.GetAllUserTasksRequest{Token: token})
	if err != nil {
		t.Fatalf("list tasks: %v", err)
	}
	if got := len(listResp.GetTasks()); got != taskCount {
		t.Fatalf("expected %d tasks, got %d", taskCount, got)
	}
	for _, task := range listResp.GetTasks() {
		if task.GetStatus() != "COMPLETED" {
			t.Fatalf("task %s not completed: %s %s", task.GetTaskId(), task.GetStatus(), task.GetStatusMessage())
		}
	}
	if got := atomic.LoadInt64(&stressWorker.executeCalls); got != taskCount {
		t.Fatalf("expected %d ExecuteTask calls, got %d", taskCount, got)
	}
	t.Logf("normal user lifecycle stress passed: tasks=%d duration=%s execute_calls=%d probe_calls=%d", taskCount, time.Since(start), atomic.LoadInt64(&stressWorker.executeCalls), atomic.LoadInt64(&stressWorker.probeCalls))
}

type mockProbeFailWorkerServer struct {
	pb.UnimplementedWorkerNodeServiceServer
	executeCalls int
}

func (m *mockProbeFailWorkerServer) TaskOutput(ctx context.Context, req *pb.TaskOutputRequest) (*pb.TaskOutputResponse, error) {
	_ = ctx
	_ = req
	return nil, status.Error(codes.Unavailable, "probe failed")
}

func (m *mockProbeFailWorkerServer) ExecuteTask(ctx context.Context, req *pb.ExecuteTaskRequest) (*pb.ExecuteTaskResponse, error) {
	_ = ctx
	_ = req
	m.executeCalls++
	return &pb.ExecuteTaskResponse{Success: true, StatusMessage: "accepted"}, nil
}

func TestMasterNodeGRPC_UploadTask_DispatchesToWorker(t *testing.T) {
	t.Parallel()

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	authSrv := newUserAuthServer(db, "test-secret")

	workerLis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("worker listen: %v", err)
	}
	workerGRPC := grpc.NewServer()
	mw := &mockWorkerServer{}
	pb.RegisterWorkerNodeServiceServer(workerGRPC, mw)
	go func() { _ = workerGRPC.Serve(workerLis) }()
	t.Cleanup(func() {
		workerGRPC.Stop()
		_ = workerLis.Close()
	})

	lis := bufconn.Listen(bufSize)
	nodepoolGRPC := grpc.NewServer()
	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	masterSrv := &masterNodeServer{svc: svc, auth: authSrv, db: db, taskToWorker: make(map[string]string), tasks: make(map[string]*taskState)}
	pb.RegisterNodeManagerServiceServer(nodepoolGRPC, &nodeManagerServer{svc: svc})
	pb.RegisterMasterNodeServiceServer(nodepoolGRPC, masterSrv)
	pb.RegisterWorkerNodeServiceServer(nodepoolGRPC, &workerIngressServer{master: masterSrv})
	pb.RegisterUserServiceServer(nodepoolGRPC, authSrv)

	go func() {
		_ = nodepoolGRPC.Serve(lis)
	}()
	t.Cleanup(func() {
		nodepoolGRPC.Stop()
		_ = lis.Close()
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	conn, err := grpc.DialContext(
		ctx,
		"bufnet",
		grpc.WithContextDialer(func(context.Context, string) (net.Conn, error) {
			return lis.Dial()
		}),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		t.Fatalf("dial nodepool: %v", err)
	}
	defer conn.Close()

	userClient := pb.NewUserServiceClient(conn)
	loginResp, err := userClient.Login(ctx, &pb.LoginRequest{Username: "worker1", Password: "worker123"})
	if err != nil {
		t.Fatalf("login err: %v", err)
	}
	if !loginResp.GetSuccess() || loginResp.GetToken() == "" {
		t.Fatalf("login failed: %s", loginResp.GetStatusMessage())
	}
	token := loginResp.GetToken()

	nodeClient := pb.NewNodeManagerServiceClient(conn)
	_, err = nodeClient.RegisterWorkerNode(ctx, &pb.RegisterWorkerNodeRequest{
		Username:    "worker1",
		Ip:          workerLis.Addr().String(),
		CpuCores:    8,
		MemoryGb:    16,
		CpuScore:    100,
		GpuScore:    100,
		GpuMemoryGb: 8,
		Location:    "local",
	})
	if err != nil {
		t.Fatalf("register worker: %v", err)
	}

	masterClient := pb.NewMasterNodeServiceClient(conn)
	uploadResp, err := masterClient.UploadTask(ctx, &pb.UploadTaskRequest{
		TaskId:      "task-dispatch-1",
		Torrent:     "magnet:?xt=urn:btih:demo",
		MemoryGb:    4,
		GpuMemoryGb: 2,
		Token:       token,
	})
	if err != nil {
		t.Fatalf("UploadTask err: %v", err)
	}
	if !uploadResp.GetSuccess() {
		t.Fatalf("UploadTask failed: %s", uploadResp.GetStatusMessage())
	}
	if mw.lastExecuteReq == nil {
		t.Fatalf("worker ExecuteTask was not called")
	}
	if mw.lastExecuteReq.GetTaskId() != "task-dispatch-1" {
		t.Fatalf("unexpected task id dispatched: %s", mw.lastExecuteReq.GetTaskId())
	}

	listResp, err := masterClient.GetAllUserTasks(ctx, &pb.GetAllUserTasksRequest{Token: token})
	if err != nil {
		t.Fatalf("GetAllUserTasks err: %v", err)
	}
	if len(listResp.GetTasks()) == 0 {
		t.Fatalf("expected tasks after UploadTask")
	}

	resultResp, err := masterClient.GetTaskResult(ctx, &pb.GetTaskResultRequest{TaskId: "task-dispatch-1", Token: token})
	if err != nil {
		t.Fatalf("GetTaskResult err: %v", err)
	}
	if resultResp.GetSuccess() {
		t.Fatalf("expected result not ready for new task")
	}

	workerIngressClient := pb.NewWorkerNodeServiceClient(conn)
	resultUploadResp, err := workerIngressClient.TaskResultUpload(ctx, &pb.TaskResultUploadRequest{
		TaskId:        "task-dispatch-1",
		ResultTorrent: "magnet:?xt=urn:btih:result-1",
		Token:         token,
	})
	if err != nil {
		t.Fatalf("TaskResultUpload to nodepool err: %v", err)
	}
	if !resultUploadResp.GetSuccess() {
		t.Fatalf("TaskResultUpload to nodepool failed: %s", resultUploadResp.GetStatusMessage())
	}

	resultResp2, err := masterClient.GetTaskResult(ctx, &pb.GetTaskResultRequest{TaskId: "task-dispatch-1", Token: token})
	if err != nil {
		t.Fatalf("GetTaskResult after upload err: %v", err)
	}
	if !resultResp2.GetSuccess() {
		t.Fatalf("expected GetTaskResult success after result upload: %s", resultResp2.GetStatusMessage())
	}

	outputUploadResp, err := workerIngressClient.TaskOutputUpload(ctx, &pb.TaskOutputUploadRequest{
		TaskId: "task-dispatch-1",
		Output: "line-1\nline-2",
		Token:  token,
	})
	if err != nil {
		t.Fatalf("TaskOutputUpload to nodepool err: %v", err)
	}
	if !outputUploadResp.GetSuccess() {
		t.Fatalf("TaskOutputUpload to nodepool failed: %s", outputUploadResp.GetStatusMessage())
	}

	logResp, err := masterClient.GetTasklog(ctx, &pb.TasklogRequest{TaskId: "task-dispatch-1", Token: token})
	if err != nil {
		t.Fatalf("GetTasklog err: %v", err)
	}
	if !logResp.GetSuccess() {
		t.Fatalf("expected GetTasklog success after output upload")
	}

	usageUploadResp, err := workerIngressClient.TaskUsage(ctx, &pb.TaskUsageRequest{
		TaskId:         "task-dispatch-1",
		CpuUsage:       12.5,
		MemoryUsage:    33.3,
		GpuUsage:       44.4,
		GpuMemoryUsage: 55.5,
		Token:          token,
	})
	if err != nil {
		t.Fatalf("TaskUsage to nodepool err: %v", err)
	}
	if !usageUploadResp.GetSuccess() {
		t.Fatalf("TaskUsage to nodepool failed: %s", usageUploadResp.GetStatusMessage())
	}

	listRespUsage, err := masterClient.GetAllUserTasks(ctx, &pb.GetAllUserTasksRequest{Token: token})
	if err != nil {
		t.Fatalf("GetAllUserTasks after usage err: %v", err)
	}
	if len(listRespUsage.GetTasks()) == 0 {
		t.Fatalf("expected tasks after TaskUsage")
	}
	usageTask := listRespUsage.GetTasks()[0]
	if usageTask.GetCpuUsage() != 12.5 || usageTask.GetMemoryUsage() != 33.3 || usageTask.GetGpuUsage() != 44.4 || usageTask.GetGpuMemoryUsage() != 55.5 {
		t.Fatalf("unexpected usage values: cpu=%v mem=%v gpu=%v gpuMem=%v", usageTask.GetCpuUsage(), usageTask.GetMemoryUsage(), usageTask.GetGpuUsage(), usageTask.GetGpuMemoryUsage())
	}
	if logResp.GetLog() != "line-1\nline-2" {
		t.Fatalf("unexpected task log: %q", logResp.GetLog())
	}
	if resultResp2.GetResultTorrent() != "magnet:?xt=urn:btih:result-1" {
		t.Fatalf("unexpected result torrent: %s", resultResp2.GetResultTorrent())
	}

	stopResp, err := masterClient.StopTask(ctx, &pb.StopTaskRequest{TaskId: "task-dispatch-1", Token: token})
	if err != nil {
		t.Fatalf("StopTask err: %v", err)
	}
	if !stopResp.GetSuccess() {
		t.Fatalf("StopTask failed: %s", stopResp.GetStatusMessage())
	}
	if mw.lastStopReq == nil {
		t.Fatalf("worker StopTaskExecution was not called")
	}
	if mw.lastStopReq.GetTaskId() != "task-dispatch-1" {
		t.Fatalf("unexpected stop task id: %s", mw.lastStopReq.GetTaskId())
	}

	listResp2, err := masterClient.GetAllUserTasks(ctx, &pb.GetAllUserTasksRequest{Token: token})
	if err != nil {
		t.Fatalf("GetAllUserTasks after stop err: %v", err)
	}
	if len(listResp2.GetTasks()) == 0 {
		t.Fatalf("expected tasks after stop")
	}
	if listResp2.GetTasks()[0].GetTaskId() != "task-dispatch-1" {
		t.Fatalf("unexpected task id in list: %s", listResp2.GetTasks()[0].GetTaskId())
	}
}

func TestMasterNode_SetTaskResult_StrictBTIH(t *testing.T) {
	t.Parallel()

	const expected = "0123456789abcdef0123456789abcdef01234567"
	m := &masterNodeServer{
		strictBTIH:   true,
		taskToWorker: map[string]string{},
		tasks: map[string]*taskState{
			"t1": {
				TaskID:       "t1",
				Owner:        "worker1",
				ExpectedBTIH: expected,
			},
		},
	}

	if ok, msg := m.setTaskResult("t1", "result://t1?src=raw"); ok || msg != "result missing btih" {
		t.Fatalf("expected strict missing btih rejection, got ok=%v msg=%q", ok, msg)
	}
	if st, _ := m.getTask("t1"); st.Status != "FAILED" {
		t.Fatalf("expected FAILED after missing btih, got %q", st.Status)
	}

	if ok, msg := m.setTaskResult("t1", "result://t1?btih=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"); ok || msg != "btih mismatch" {
		t.Fatalf("expected btih mismatch rejection, got ok=%v msg=%q", ok, msg)
	}

	if ok, msg := m.setTaskResult("t1", "result://t1?btih="+expected); !ok || msg != "uploaded" {
		t.Fatalf("expected uploaded with matching btih, got ok=%v msg=%q", ok, msg)
	}
	if st, _ := m.getTask("t1"); st.Status != "COMPLETED" {
		t.Fatalf("expected COMPLETED after match, got %q", st.Status)
	}
}

func TestMasterNode_DispatchTaskToWorker_PreDispatchProbe(t *testing.T) {
	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	m := &masterNodeServer{svc: svc, taskToWorker: map[string]string{}, tasks: map[string]*taskState{}}

	workerLis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("worker listen: %v", err)
	}
	workerGRPC := grpc.NewServer()
	mw := &mockProbeFailWorkerServer{}
	pb.RegisterWorkerNodeServiceServer(workerGRPC, mw)
	go func() { _ = workerGRPC.Serve(workerLis) }()
	t.Cleanup(func() {
		workerGRPC.Stop()
		_ = workerLis.Close()
	})

	if err := svc.RegisterWorker(context.Background(), &repository.Worker{ID: "w-probe", Addr: workerLis.Addr().String()}); err != nil {
		t.Fatalf("register worker: %v", err)
	}

	task := &taskState{TaskID: "task-probe-1", TorrentSource: "magnet:?xt=urn:btih:demo", ReqMemoryGB: 1, ReqGPUMemoryGB: 1}

	t.Setenv("NODEPOOL_PRE_DISPATCH_PROBE", "true")
	_, _, reason, ok := m.dispatchTaskToWorker(context.Background(), task, "")
	if ok {
		t.Fatalf("expected dispatch to fail when pre-dispatch probe fails")
	}
	if reason == "" || !strings.Contains(reason, "probe") {
		t.Fatalf("expected probe-related reason, got %q", reason)
	}
	if mw.executeCalls != 0 {
		t.Fatalf("expected ExecuteTask not called when probe fails, calls=%d", mw.executeCalls)
	}

	t.Setenv("NODEPOOL_PRE_DISPATCH_PROBE", "false")
	if _, _, _, ok := m.dispatchTaskToWorker(context.Background(), task, ""); !ok {
		t.Fatalf("expected dispatch to succeed when pre-dispatch probe is disabled")
	}
	if mw.executeCalls == 0 {
		t.Fatalf("expected ExecuteTask called when probe disabled")
	}
}

func TestFormatDispatchReason(t *testing.T) {
	t.Parallel()

	tests := []struct {
		in   string
		want string
	}{
		{in: "", want: "[NO_WORKER] no available worker"},
		{in: "worker probe failed at 127.0.0.1:50053", want: "[PROBE_FAIL] worker probe failed at 127.0.0.1:50053"},
		{in: "dial worker 127.0.0.1 failed", want: "[DIAL_FAIL] dial worker 127.0.0.1 failed"},
		{in: "execute rpc failed at 127.0.0.1", want: "[EXEC_FAIL] execute rpc failed at 127.0.0.1"},
		{in: "worker rejected task at 127.0.0.1", want: "[REJECTED] worker rejected task at 127.0.0.1"},
	}

	for _, tc := range tests {
		got := formatDispatchReason(tc.in)
		if got != tc.want {
			t.Fatalf("formatDispatchReason(%q)=%q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestMasterNode_SetTaskResult_SettlesCPT(t *testing.T) {
	t.Parallel()

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	const expected = "0123456789abcdef0123456789abcdef01234567"
	m := &masterNodeServer{
		db:           db,
		taskToWorker: map[string]string{},
		tasks: map[string]*taskState{
			"t1": {
				TaskID:         "t1",
				Owner:          "worker1",
				WorkerID:       "worker2",
				ExpectedBTIH:   expected,
				ReqCPUScore:    100,
				ReqMemoryGB:    1,
				ReqGPUScore:    0,
				ReqGPUMemoryGB: 0,
				HostCount:      1,
			},
		},
	}

	if ok, msg := m.setTaskResult("t1", "result://t1?btih="+expected); !ok || msg != "uploaded" {
		t.Fatalf("expected uploaded with settlement, got ok=%v msg=%q", ok, msg)
	}

	var b1, b2 int64
	if err := db.QueryRow("SELECT balance FROM users WHERE username='worker1'").Scan(&b1); err != nil {
		t.Fatalf("query worker1 balance: %v", err)
	}
	if err := db.QueryRow("SELECT balance FROM users WHERE username='worker2'").Scan(&b2); err != nil {
		t.Fatalf("query worker2 balance: %v", err)
	}
	if b1 != 998 || b2 != 802 {
		t.Fatalf("unexpected balances after settle, worker1=%d worker2=%d", b1, b2)
	}

	st, ok := m.getTask("t1")
	if !ok {
		t.Fatalf("task t1 not found")
	}
	if !st.BillingSettled || st.BilledAmount != 2 {
		t.Fatalf("expected settled amount=2, got settled=%v amount=%d", st.BillingSettled, st.BilledAmount)
	}
}

func TestMasterNode_SetTaskResult_SettlementInsufficientBalance(t *testing.T) {
	t.Parallel()

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	const expected = "0123456789abcdef0123456789abcdef01234567"
	m := &masterNodeServer{
		db:           db,
		taskToWorker: map[string]string{},
		tasks: map[string]*taskState{
			"t1": {
				TaskID:         "t1",
				Owner:          "worker2",
				WorkerID:       "worker1",
				ExpectedBTIH:   expected,
				ReqCPUScore:    100,
				ReqGPUScore:    5000,
				ReqMemoryGB:    32,
				ReqGPUMemoryGB: 16,
				HostCount:      1,
			},
		},
	}

	if ok, msg := m.setTaskResult("t1", "result://t1?btih="+expected); ok || msg != "insufficient balance" {
		t.Fatalf("expected insufficient balance, got ok=%v msg=%q", ok, msg)
	}

	st, ok := m.getTask("t1")
	if !ok {
		t.Fatalf("task t1 not found")
	}
	if st.Status != "FAILED" {
		t.Fatalf("expected FAILED when settlement fails, got %q", st.Status)
	}
}

func TestMasterNode_SetTaskResult_NonStrict_AllowsMissingBTIH(t *testing.T) {
	t.Parallel()

	const expected = "0123456789abcdef0123456789abcdef01234567"
	m := &masterNodeServer{
		strictBTIH:   false,
		taskToWorker: map[string]string{},
		tasks: map[string]*taskState{
			"t1": {
				TaskID:       "t1",
				Owner:        "worker1",
				ExpectedBTIH: expected,
			},
		},
	}

	if ok, msg := m.setTaskResult("t1", "result://t1?src=raw"); !ok || msg != "uploaded" {
		t.Fatalf("expected non-strict upload accepted, got ok=%v msg=%q", ok, msg)
	}
	if st, _ := m.getTask("t1"); st.Status != "COMPLETED" {
		t.Fatalf("expected COMPLETED in non-strict mode, got %q", st.Status)
	}
}

func TestExtractStrictBTIHFromSource(t *testing.T) {
	t.Parallel()

	if h, err := extractStrictBTIHFromSource("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"); err != nil || h != "0123456789abcdef0123456789abcdef01234567" {
		t.Fatalf("magnet strict parse failed, h=%q err=%v", h, err)
	}
	if h, err := extractStrictBTIHFromSource("https://example.com/torrents/a.torrent?ih=89abcdef0123456789abcdef0123456789abcdef"); err != nil || h != "89abcdef0123456789abcdef0123456789abcdef" {
		t.Fatalf("http strict parse failed, h=%q err=%v", h, err)
	}

	if _, err := extractStrictBTIHFromSource("magnet:?xt=urn:btih:demo"); err == nil {
		t.Fatalf("expected strict error for short btih")
	}
	if _, err := extractStrictBTIHFromSource("https://example.com/t.torrent"); err == nil {
		t.Fatalf("expected strict error for missing ih")
	}
	if _, err := extractStrictBTIHFromSource("file://local/path"); err == nil {
		t.Fatalf("expected strict error for unsupported source")
	}
}

func TestMasterNode_UploadTask_StrictRejectsInvalidSource(t *testing.T) {
	t.Parallel()

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	authSrv := newUserAuthServer(db, "test-secret")
	loginResp, err := authSrv.Login(context.Background(), &pb.LoginRequest{Username: "worker1", Password: "worker123"})
	if err != nil {
		t.Fatalf("login err: %v", err)
	}
	if !loginResp.GetSuccess() || loginResp.GetToken() == "" {
		t.Fatalf("login failed: %s", loginResp.GetStatusMessage())
	}

	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	m := &masterNodeServer{svc: svc, auth: authSrv, db: db, strictBTIH: true, taskToWorker: make(map[string]string), tasks: make(map[string]*taskState)}

	resp, err := m.UploadTask(context.Background(), &pb.UploadTaskRequest{
		TaskId:      "strict-src-1",
		Torrent:     "magnet:?xt=urn:btih:demo",
		MemoryGb:    2,
		GpuMemoryGb: 1,
		Token:       loginResp.GetToken(),
	})
	if err != nil {
		t.Fatalf("UploadTask err: %v", err)
	}
	if resp.GetSuccess() {
		t.Fatalf("expected strict mode to reject invalid source")
	}
	if resp.GetStatusMessage() != "invalid torrent source btih" {
		t.Fatalf("unexpected strict rejection message: %q", resp.GetStatusMessage())
	}
}

func TestMasterNode_UploadTask_DispatchFailureWritesTaskLog(t *testing.T) {
	t.Parallel()

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	authSrv := newUserAuthServer(db, "test-secret")
	loginResp, err := authSrv.Login(context.Background(), &pb.LoginRequest{Username: "worker1", Password: "worker123"})
	if err != nil {
		t.Fatalf("login err: %v", err)
	}
	if !loginResp.GetSuccess() || loginResp.GetToken() == "" {
		t.Fatalf("login failed: %s", loginResp.GetStatusMessage())
	}

	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	m := &masterNodeServer{svc: svc, auth: authSrv, db: db, strictBTIH: false, taskToWorker: make(map[string]string), tasks: make(map[string]*taskState)}

	resp, err := m.UploadTask(context.Background(), &pb.UploadTaskRequest{
		TaskId:      "no-worker-1",
		Torrent:     "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
		MemoryGb:    2,
		GpuMemoryGb: 1,
		Token:       loginResp.GetToken(),
	})
	if err != nil {
		t.Fatalf("UploadTask err: %v", err)
	}
	if resp.GetSuccess() {
		t.Fatalf("expected UploadTask to fail without workers")
	}
	if !strings.Contains(resp.GetStatusMessage(), "[NO_WORKER]") {
		t.Fatalf("expected NO_WORKER code in status message, got %q", resp.GetStatusMessage())
	}

	logResp, err := m.GetTasklog(context.Background(), &pb.TasklogRequest{TaskId: "no-worker-1", Token: loginResp.GetToken()})
	if err != nil {
		t.Fatalf("GetTasklog err: %v", err)
	}
	if !logResp.GetSuccess() {
		t.Fatalf("expected task log available after dispatch failure, got %q", logResp.GetLog())
	}
	if !strings.Contains(logResp.GetLog(), "[NO_WORKER]") {
		t.Fatalf("expected NO_WORKER code in task log, got %q", logResp.GetLog())
	}
}

func TestMasterNode_ProcessPeriodicSettlements(t *testing.T) {
	t.Parallel()

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	m := &masterNodeServer{
		db:           db,
		taskToWorker: map[string]string{},
		tasks: map[string]*taskState{
			"t1": {
				TaskID:         "t1",
				Owner:          "worker1",
				WorkerID:       "worker2",
				Status:         "RUNNING",
				ReqCPUScore:    100,
				ReqMemoryGB:    1,
				ReqGPUScore:    0,
				ReqGPUMemoryGB: 0,
				HostCount:      1,
			},
		},
	}

	m.processPeriodicSettlements(time.Minute)

	st, ok := m.getTask("t1")
	if !ok {
		t.Fatalf("task not found")
	}
	if st.BilledAmount != 2 {
		t.Fatalf("expected billed amount 2 after periodic settle, got %d", st.BilledAmount)
	}

	var b1, b2 int64
	if err := db.QueryRow("SELECT balance FROM users WHERE username='worker1'").Scan(&b1); err != nil {
		t.Fatalf("query worker1 balance: %v", err)
	}
	if err := db.QueryRow("SELECT balance FROM users WHERE username='worker2'").Scan(&b2); err != nil {
		t.Fatalf("query worker2 balance: %v", err)
	}
	if b1 != 998 || b2 != 802 {
		t.Fatalf("unexpected balances after periodic settle, worker1=%d worker2=%d", b1, b2)
	}

	// within interval, no extra settle
	m.processPeriodicSettlements(time.Hour)
	var b1b, b2b int64
	if err := db.QueryRow("SELECT balance FROM users WHERE username='worker1'").Scan(&b1b); err != nil {
		t.Fatalf("query worker1 balance 2nd: %v", err)
	}
	if err := db.QueryRow("SELECT balance FROM users WHERE username='worker2'").Scan(&b2b); err != nil {
		t.Fatalf("query worker2 balance 2nd: %v", err)
	}
	if b1b != b1 || b2b != b2 {
		t.Fatalf("expected no additional settlement in interval, got worker1=%d worker2=%d", b1b, b2b)
	}
}

func TestMasterNode_ProcessPeriodicSettlements_InsufficientBalanceFailsTask(t *testing.T) {
	t.Parallel()

	workerLis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("worker listen: %v", err)
	}
	workerGRPC := grpc.NewServer()
	mw := &mockWorkerServer{}
	pb.RegisterWorkerNodeServiceServer(workerGRPC, mw)
	go func() { _ = workerGRPC.Serve(workerLis) }()
	t.Cleanup(func() {
		workerGRPC.Stop()
		_ = workerLis.Close()
	})

	db, err := initDB(testPostgresDSN(t))
	if err != nil {
		t.Fatalf("init db: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	m := &masterNodeServer{
		db:           db,
		taskToWorker: map[string]string{"t2": workerLis.Addr().String()},
		tasks: map[string]*taskState{
			"t2": {
				TaskID:         "t2",
				Owner:          "worker2",
				WorkerID:       "worker1",
				WorkerIP:       workerLis.Addr().String(),
				Status:         "RUNNING",
				ReqCPUScore:    100,
				ReqGPUScore:    5000,
				ReqMemoryGB:    32,
				ReqGPUMemoryGB: 16,
				HostCount:      1,
			},
		},
	}

	m.processPeriodicSettlements(time.Second)

	st, ok := m.getTask("t2")
	if !ok {
		t.Fatalf("task not found")
	}
	if st.Status != "FAILED" {
		t.Fatalf("expected FAILED on insufficient balance, got %q", st.Status)
	}
	if !st.BillingSettled {
		t.Fatalf("expected billing settled after failure")
	}
	if mw.lastStopReq == nil || mw.lastStopReq.GetTaskId() != "t2" {
		t.Fatalf("expected stop request to worker on insufficient balance")
	}
}

func TestMasterNode_SetTaskOutput_ProgramErrorMarksFailed(t *testing.T) {
	t.Parallel()

	m := &masterNodeServer{
		taskToWorker: map[string]string{"t1": "127.0.0.1:50053"},
		tasks: map[string]*taskState{
			"t1": {
				TaskID:   "t1",
				Owner:    "worker1",
				WorkerIP: "127.0.0.1:50053",
				Status:   "RUNNING",
			},
		},
	}

	if ok := m.setTaskOutput("t1", "task failed: python exception"); !ok {
		t.Fatalf("setTaskOutput should succeed")
	}
	st, ok := m.getTask("t1")
	if !ok {
		t.Fatalf("task not found")
	}
	if st.Status != "FAILED" {
		t.Fatalf("expected FAILED, got %q", st.Status)
	}
	if _, routed := m.getTaskRoute("t1"); routed {
		t.Fatalf("route should be removed on program error")
	}
}

func TestMasterNode_ProcessTaskTimeouts_Redispatch(t *testing.T) {
	t.Parallel()

	workerLis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("worker listen: %v", err)
	}
	workerGRPC := grpc.NewServer()
	mw := &mockWorkerServer{}
	pb.RegisterWorkerNodeServiceServer(workerGRPC, mw)
	go func() { _ = workerGRPC.Serve(workerLis) }()
	t.Cleanup(func() {
		workerGRPC.Stop()
		_ = workerLis.Close()
	})

	repo := repository.NewWorkerRepository()
	svc := service.NewWorkerService(repo)
	if err := svc.RegisterWorker(context.Background(), &repository.Worker{ID: "w2", Addr: workerLis.Addr().String()}); err != nil {
		t.Fatalf("register worker: %v", err)
	}

	m := &masterNodeServer{
		svc:          svc,
		taskToWorker: map[string]string{"t-timeout": "127.0.0.1:1"},
		tasks: map[string]*taskState{
			"t-timeout": {
				TaskID:         "t-timeout",
				Owner:          "worker1",
				WorkerID:       "old-worker",
				WorkerIP:       "127.0.0.1:1",
				Status:         "DISPATCHED",
				TorrentSource:  "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
				ReqMemoryGB:    2,
				ReqGPUMemoryGB: 1,
				LastUpdate:     time.Now().Add(-2 * time.Minute),
			},
		},
	}

	m.processTaskTimeouts(1*time.Second, 2)

	st, ok := m.getTask("t-timeout")
	if !ok {
		t.Fatalf("task not found")
	}
	if st.Status != "DISPATCHED" {
		t.Fatalf("expected DISPATCHED after redispatch, got %q", st.Status)
	}
	if st.WorkerIP != workerLis.Addr().String() {
		t.Fatalf("expected redispatched worker addr %q, got %q", workerLis.Addr().String(), st.WorkerIP)
	}
	if st.RetryCount != 1 {
		t.Fatalf("expected retry_count=1, got %d", st.RetryCount)
	}
	if mw.lastExecuteReq == nil || mw.lastExecuteReq.GetTaskId() != "t-timeout" {
		t.Fatalf("expected worker ExecuteTask called for timeout redispatch")
	}
}

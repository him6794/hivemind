package main

import (
	"context"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/test/bufconn"

	"hivemind/services/worker/internal/service"
	"hivemind/services/worker/pb"
)

const bufSize = 1024 * 1024

func TestWorkerNodeGRPC_BasicFlow(t *testing.T) {
	t.Parallel()

	lis := bufconn.Listen(bufSize)
	grpcServer := grpc.NewServer()
	pb.RegisterWorkerNodeServiceServer(grpcServer, &workerServer{svc: service.NewTaskService()})

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

	client := pb.NewWorkerNodeServiceClient(conn)

	_, err = client.ExecuteTask(ctx, &pb.ExecuteTaskRequest{
		TaskId:      "task-1",
		Torrent:     "magnet:?xt=urn:btih:demo",
		MemoryGb:    4,
		GpuMemoryGb: 2,
	})
	if err != nil {
		t.Fatalf("ExecuteTask err: %v", err)
	}

	_, err = client.TaskOutputUpload(ctx, &pb.TaskOutputUploadRequest{
		TaskId: "task-1",
		Output: "hello output",
	})
	if err != nil {
		t.Fatalf("TaskOutputUpload err: %v", err)
	}

	outResp, err := client.TaskOutput(ctx, &pb.TaskOutputRequest{TaskId: "task-1"})
	if err != nil {
		t.Fatalf("TaskOutput err: %v", err)
	}
	if !outResp.GetSuccess() || outResp.GetOutput() != "hello output" {
		t.Fatalf("unexpected TaskOutput response: success=%v output=%q", outResp.GetSuccess(), outResp.GetOutput())
	}

	_, err = client.TaskUsage(ctx, &pb.TaskUsageRequest{
		TaskId:         "task-1",
		CpuUsage:       0.4,
		MemoryUsage:    0.5,
		GpuUsage:       0.1,
		GpuMemoryUsage: 0.2,
	})
	if err != nil {
		t.Fatalf("TaskUsage err: %v", err)
	}

	_, err = client.StopTaskExecution(ctx, &pb.StopTaskExecutionRequest{TaskId: "task-1"})
	if err != nil {
		t.Fatalf("StopTaskExecution err: %v", err)
	}

	resResp, err := client.TaskResultUpload(ctx, &pb.TaskResultUploadRequest{
		TaskId:        "task-1",
		ResultTorrent: "magnet:?xt=urn:btih:result",
	})
	if err != nil {
		t.Fatalf("TaskResultUpload err: %v", err)
	}
	if !resResp.GetSuccess() {
		t.Fatalf("TaskResultUpload failed: %s", resResp.GetStatusMessage())
	}

	missingResp, err := client.TaskOutput(ctx, &pb.TaskOutputRequest{TaskId: "missing-task"})
	if err != nil {
		t.Fatalf("TaskOutput missing task should return app error response, got rpc error: %v", err)
	}
	if missingResp.GetSuccess() {
		t.Fatalf("expected TaskOutput missing task to fail")
	}
}

func TestParseExecutorResult(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		in   string
		want string
	}{
		{
			name: "result_torrent key",
			in:   "log line\nRESULT_TORRENT=magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\n",
			want: "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
		},
		{
			name: "direct result url",
			in:   "progress 10%\nresult://task-1?btih=0123456789abcdef0123456789abcdef01234567",
			want: "result://task-1?btih=0123456789abcdef0123456789abcdef01234567",
		},
		{
			name: "empty",
			in:   "only log\nno result",
			want: "",
		},
	}

	for _, tc := range tests {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			got := parseExecutorResult(tc.in)
			if got != tc.want {
				t.Fatalf("parseExecutorResult() = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestExecutorOutputLines(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		in   string
		want []string
	}{
		{
			name: "keep print lines and drop control line",
			in:   "hello\nRESULT_TORRENT=result://task-1?btih=0123456789abcdef0123456789abcdef01234567\nworld\n",
			want: []string{"hello", "world"},
		},
		{
			name: "trim and skip empty",
			in:   "\r\n  line-1  \r\n\r\n line-2 \n",
			want: []string{"line-1", "line-2"},
		},
	}

	for _, tc := range tests {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			got := executorOutputLines(tc.in)
			if len(got) != len(tc.want) {
				t.Fatalf("executorOutputLines() len=%d, want=%d, got=%v", len(got), len(tc.want), got)
			}
			for i := range tc.want {
				if got[i] != tc.want[i] {
					t.Fatalf("executorOutputLines()[%d]=%q, want %q", i, got[i], tc.want[i])
				}
			}
		})
	}
}

func TestClampPercent(t *testing.T) {
	t.Parallel()

	if got := clampPercent(-1); got != 0 {
		t.Fatalf("clampPercent(-1)=%v, want 0", got)
	}
	if got := clampPercent(55.5); got != 55.5 {
		t.Fatalf("clampPercent(55.5)=%v, want 55.5", got)
	}
	if got := clampPercent(101); got != 100 {
		t.Fatalf("clampPercent(101)=%v, want 100", got)
	}
}

func TestGenerateUsageSample_Bounds(t *testing.T) {
	t.Parallel()

	r := &workerRuntime{profile: workerProfile{MemoryGb: 16, GpuScore: 100, GpuMemoryGb: 8}}
	cpu, mem, gpu, gpuMem := r.generateUsageSample("task-usage-1")

	if cpu < 0 || cpu > 100 {
		t.Fatalf("cpu out of range: %v", cpu)
	}
	if mem < 0 || mem > 100 {
		t.Fatalf("mem out of range: %v", mem)
	}
	if gpu < 0 || gpu > 100 {
		t.Fatalf("gpu out of range: %v", gpu)
	}
	if gpuMem < 0 || gpuMem > 100 {
		t.Fatalf("gpuMem out of range: %v", gpuMem)
	}
}

func TestResolveExecutorCommand(t *testing.T) {
	t.Run("explicit command wins", func(t *testing.T) {
		t.Setenv("WORKER_EXECUTOR_CMD", "custom-exec --flag")
		t.Setenv("WORKER_EXECUTOR_AUTO_RUST", "0")
		got := resolveExecutorCommand()
		if got != "custom-exec --flag" {
			t.Fatalf("resolveExecutorCommand()=%q, want explicit command", got)
		}
	})

	t.Run("auto rust default binary", func(t *testing.T) {
		t.Setenv("WORKER_EXECUTOR_CMD", "")
		t.Setenv("WORKER_EXECUTOR_AUTO_RUST", "1")
		t.Setenv("WORKER_EXECUTOR_RS_BIN", "")
		got := resolveExecutorCommand()
		want := defaultExecutorCommand()
		if got != want {
			t.Fatalf("resolveExecutorCommand()=%q, want %q", got, want)
		}
	})

	t.Run("default prefers repo monty", func(t *testing.T) {
		got := defaultExecutorCommand()
		if _, err := os.Stat(got); err == nil && filepath.Base(got) != "monty.exe" && filepath.Base(got) != "monty" {
			t.Fatalf("defaultExecutorCommand()=%q, want repo monty when available", got)
		}
	})

	t.Run("auto rust disabled", func(t *testing.T) {
		t.Setenv("WORKER_EXECUTOR_CMD", "")
		t.Setenv("WORKER_EXECUTOR_AUTO_RUST", "false")
		t.Setenv("WORKER_EXECUTOR_RS_BIN", "executor-cli")
		got := resolveExecutorCommand()
		if got != "" {
			t.Fatalf("resolveExecutorCommand()=%q, want empty", got)
		}
	})
}

func TestExecutorProgramFromCommand(t *testing.T) {
	t.Parallel()

	t.Run("ok", func(t *testing.T) {
		got, err := executorProgramFromCommand("executor-cli --verbose")
		if err != nil {
			t.Fatalf("unexpected err: %v", err)
		}
		if got != "executor-cli" {
			t.Fatalf("program=%q, want executor-cli", got)
		}
	})

	t.Run("empty", func(t *testing.T) {
		if _, err := executorProgramFromCommand("   "); err == nil {
			t.Fatalf("expected error for empty command")
		}
	})
}

func TestMontyResultScript(t *testing.T) {
	t.Parallel()

	const expected = "0123456789abcdef0123456789abcdef01234567"

	script, result := montyResultScript("task-1", "magnet:?xt=urn:btih:"+expected+"&dn=test")
	if result == "" || script == "" {
		t.Fatalf("montyResultScript returned empty values: script=%q result=%q", script, result)
	}
	if want := "result://task-1?btih=" + expected; result != want {
		t.Fatalf("montyResultScript magnet result=%q, want %q", result, want)
	}
	if got := parseExecutorResult("log\nRESULT_TORRENT=" + result); got != result {
		t.Fatalf("parseExecutorResult()=%q, want %q", got, result)
	}

	_, result = montyResultScript("task-2", "http://localhost:8082/api/torrents/"+expected+".torrent?ih="+expected)
	if want := "result://task-2?btih=" + expected; result != want {
		t.Fatalf("montyResultScript torrent URL result=%q, want %q", result, want)
	}

	_, fallback := montyResultScript("task-3", "http://localhost/test.torrent")
	if fallback == "" || fallback == "result://task-3?btih="+expected {
		t.Fatalf("montyResultScript fallback result looks wrong: %q", fallback)
	}
}

func TestBTIHFromTorrentSource(t *testing.T) {
	t.Parallel()

	const expected = "0123456789abcdef0123456789abcdef01234567"
	tests := []struct {
		name string
		in   string
		want string
	}{
		{name: "magnet", in: "magnet:?xt=urn:btih:" + expected + "&dn=test", want: expected},
		{name: "torrent url ih", in: "http://localhost/test.torrent?ih=" + expected, want: expected},
		{name: "torrent url btih", in: "https://example.test/test.torrent?btih=" + expected, want: expected},
		{name: "invalid", in: "magnet:?xt=urn:btih:nothex", want: ""},
	}

	for _, tc := range tests {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if got := btihFromTorrentSource(tc.in); got != tc.want {
				t.Fatalf("btihFromTorrentSource()=%q, want %q", got, tc.want)
			}
		})
	}
}

func TestIsMontyExecutor(t *testing.T) {
	t.Parallel()

	if !isMontyExecutor(`D:\hivemind\executor-rs\monty.exe`) {
		t.Fatalf("expected monty.exe path to be detected")
	}
	if isMontyExecutor("executor-cli") {
		t.Fatalf("executor-cli should not be detected as monty")
	}
}

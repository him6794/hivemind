package main

import (
	"bytes"
	"strings"
	"testing"
)

func TestTaskKindClassifiesWorkloadFromTaskID(t *testing.T) {
	tests := []struct {
		taskID string
		want   string
	}{
		{"rel-r01-failure", "failure"},
		{"rel-r01-long", "long"},
		{"rel-r01-retry", "retry"},
		{"rel-r01-io", "io"},
		{"rel-r01-parallel-1", "parallel"},
		{"rel-r01-cpu", "cpu"},
	}

	for _, tc := range tests {
		if got := taskKind(tc.taskID); got != tc.want {
			t.Fatalf("taskKind(%q)=%q, want %q", tc.taskID, got, tc.want)
		}
	}
}

func TestBTIHFromTorrentPrefersMagnetHash(t *testing.T) {
	const expected = "0123456789abcdef0123456789abcdef01234567"
	got := btihFromTorrent("task-1", "magnet:?xt=urn:btih:"+expected+"&dn=test")
	if got != expected {
		t.Fatalf("btih=%q, want %q", got, expected)
	}
}

func TestRunCompletesCPUWorkloadAndPrintsResultTorrent(t *testing.T) {
	t.Setenv("RELIABILITY_CPU_SECONDS", "0")
	var stdout bytes.Buffer
	code := run([]string{"reliability-executor", "rel-r01-cpu", "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=cpu"}, &stdout)
	if code != 0 {
		t.Fatalf("run exit=%d, want 0; stdout=%s", code, stdout.String())
	}
	out := stdout.String()
	if !strings.Contains(out, "executor_complete task_id=rel-r01-cpu kind=cpu") {
		t.Fatalf("stdout missing completion line: %s", out)
	}
	if !strings.Contains(out, "RESULT_TORRENT=result://rel-r01-cpu?btih=0123456789abcdef0123456789abcdef01234567") {
		t.Fatalf("stdout missing result torrent: %s", out)
	}
}

func TestRunFailsInjectedFailureWorkload(t *testing.T) {
	t.Setenv("RELIABILITY_FAILURE_SECONDS", "0")
	var stdout bytes.Buffer
	code := run([]string{"reliability-executor", "rel-r01-failure", "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=failure"}, &stdout)
	if code != 42 {
		t.Fatalf("run exit=%d, want 42; stdout=%s", code, stdout.String())
	}
	if !strings.Contains(stdout.String(), "executor_failure_injected kind=failure") {
		t.Fatalf("stdout missing injected failure line: %s", stdout.String())
	}
}

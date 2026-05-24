package main

import (
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

func envInt(name string, fallback int) int {
	raw := strings.TrimSpace(os.Getenv(name))
	if raw == "" {
		return fallback
	}
	value, err := strconv.Atoi(raw)
	if err != nil || value < 0 {
		return fallback
	}
	return value
}

func taskKind(taskID string) string {
	lowered := strings.ToLower(taskID)
	switch {
	case strings.Contains(lowered, "failure"), strings.Contains(lowered, "fail"):
		return "failure"
	case strings.Contains(lowered, "long"):
		return "long"
	case strings.Contains(lowered, "retry"):
		return "retry"
	case strings.Contains(lowered, "io"):
		return "io"
	case strings.Contains(lowered, "parallel"):
		return "parallel"
	default:
		return "cpu"
	}
}

func btihFromTorrent(taskID, torrent string) string {
	parsed, err := url.Parse(strings.TrimSpace(torrent))
	if err == nil && strings.EqualFold(parsed.Scheme, "magnet") {
		for _, xt := range parsed.Query()["xt"] {
			if len(xt) >= len("urn:btih:") && strings.EqualFold(xt[:len("urn:btih:")], "urn:btih:") {
				candidate := strings.ToLower(strings.TrimSpace(xt[len("urn:btih:"):]))
				if len(candidate) == 40 {
					return candidate
				}
			}
		}
	}
	sum := sha1.Sum([]byte(taskID + "|" + torrent))
	return hex.EncodeToString(sum[:])
}

func sleepWithProgress(seconds int, label string, out io.Writer) {
	deadline := time.Now().Add(time.Duration(seconds) * time.Second)
	nextLog := time.Now()
	for {
		remaining := time.Until(deadline)
		if remaining <= 0 {
			return
		}
		if !time.Now().Before(nextLog) {
			elapsed := seconds - int(remaining.Seconds())
			if elapsed < 0 {
				elapsed = 0
			}
			fmt.Fprintf(out, "executor_progress kind=%s elapsed_sec=%d target_sec=%d\n", label, elapsed, seconds)
			nextLog = time.Now().Add(30 * time.Second)
		}
		if remaining > time.Second {
			remaining = time.Second
		}
		time.Sleep(remaining)
	}
}

func cpuWork(seconds int) string {
	deadline := time.Now().Add(time.Duration(seconds) * time.Second)
	digest := []byte("seed")
	rounds := 0
	for !time.Now().After(deadline) {
		sum := sha256.Sum256(append(digest, []byte(strconv.Itoa(rounds))...))
		digest = sum[:]
		rounds++
	}
	sum := sha1.Sum(digest)
	return hex.EncodeToString(sum[:])
}

func ioWork(taskID string, seconds int) string {
	file, err := os.CreateTemp("", "hivemind-reliability-"+taskID+"-*.bin")
	if err != nil {
		return cpuWork(0)
	}
	path := file.Name()
	defer os.Remove(path)
	defer file.Close()

	base := sha256.Sum256([]byte(taskID))
	payload := make([]byte, 0, len(base)*4096)
	for i := 0; i < 4096; i++ {
		payload = append(payload, base[:]...)
	}

	deadline := time.Now().Add(time.Duration(seconds) * time.Second)
	writes := 0
	for !time.Now().After(deadline) {
		_, _ = file.Write(payload)
		writes++
	}
	_ = file.Close()

	readFile, err := os.Open(path)
	if err != nil {
		return cpuWork(0)
	}
	defer readFile.Close()
	digest := sha1.New()
	_, _ = io.Copy(digest, readFile)
	_, _ = digest.Write([]byte(strconv.Itoa(writes)))
	return hex.EncodeToString(digest.Sum(nil))
}

func run(argv []string, out io.Writer) int {
	if len(argv) < 3 {
		fmt.Fprintln(out, "executor_error missing task_id/torrent")
		return 64
	}
	taskID := argv[1]
	torrent := argv[2]
	kind := taskKind(taskID)
	start := time.Now()

	fmt.Fprintf(out, "executor_start task_id=%s kind=%s\n", taskID, kind)

	var workHash string
	switch kind {
	case "failure":
		sleepWithProgress(envInt("RELIABILITY_FAILURE_SECONDS", 3), kind, out)
		fmt.Fprintln(out, "executor_failure_injected kind=failure")
		return 42
	case "long":
		sleepWithProgress(envInt("RELIABILITY_LONG_SECONDS", 900), kind, out)
		workHash = cpuWork(envInt("RELIABILITY_LONG_CPU_TAIL_SECONDS", 2))
	case "retry":
		sleepWithProgress(envInt("RELIABILITY_RETRY_SECONDS", 90), kind, out)
		workHash = cpuWork(envInt("RELIABILITY_RETRY_CPU_TAIL_SECONDS", 2))
	case "io":
		workHash = ioWork(taskID, envInt("RELIABILITY_IO_SECONDS", 5))
	case "parallel":
		sleepWithProgress(envInt("RELIABILITY_PARALLEL_SECONDS", 10), kind, out)
		workHash = cpuWork(envInt("RELIABILITY_PARALLEL_CPU_TAIL_SECONDS", 1))
	default:
		workHash = cpuWork(envInt("RELIABILITY_CPU_SECONDS", 5))
	}

	btih := btihFromTorrent(taskID, torrent)
	result := "result://" + url.QueryEscape(taskID) + "?btih=" + btih
	fmt.Fprintf(out, "executor_complete task_id=%s kind=%s elapsed_sec=%.3f work_hash=%s\n", taskID, kind, time.Since(start).Seconds(), workHash)
	fmt.Fprintf(out, "RESULT_TORRENT=%s\n", result)
	return 0
}

func main() {
	os.Exit(run(os.Args, os.Stdout))
}
